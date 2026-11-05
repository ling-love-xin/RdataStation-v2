// ==================== 详情视图（中央 tab：字段 + 预览） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 样例预览的固定种子：样例稳定可复现（与最终生成的随机种子无关，纯示意）
use fake::rand::SeedableRng;
use fake::rand::rngs::StdRng;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::separator::Separator;

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

    /// 列编辑对话框的「应用」：把工作副本合成为列规格（读全部输入框现值）。
    fn collect_column_edit(&mut self, cx: &App) -> Option<MockColumnSpec> {
        let draft = self.draft.take()?;
        let name = draft.name_input.read(cx).value().trim().to_string();
        let ratio = parse_percent_ratio(&draft.null_input.read(cx).value())
            .unwrap_or(draft.def.nullable_ratio);
        let mut def = draft.def.clone();
        def.name = name;
        def.nullable_ratio = ratio;
        Some(MockColumnSpec {
            id: draft.id,
            def,
            confidence: draft.confidence.clone(),
            sample_value: draft.sample_value.clone(),
            // 自定义项身份随列一起写回：左栏精确高亮 / 右栏显示自定义名都靠它
            user_generator: draft.picked_user.clone(),
        })
    }

    /// #7 应用并新增下一列：应用当前表配置 → 按当前列名 +2 去重新建列（沿用 §8.3 字段预设命名策略）
    /// → 立即打开新列的列编辑对话框（连续录入）。旧对话框的关闭由按钮回调先执行
    ///（`close_dialog` 需要 `&mut App`），本方法只做「应用 + 加列 + 重开对话框」。
    pub(crate) fn apply_and_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 先把活动指针切到这张 tab 的配置：下面的应用 / 加列都按「当前配置」写
        let config_id = self.config_id();
        // 先取当前列名与类型（collect 会 take 表配置，之后读不到）
        let (base_name, base_type) = self
            .draft
            .as_ref()
            .map(|d| {
                (
                    d.name_input.read(cx).value().trim().to_string(),
                    d.def.data_type.clone(),
                )
            })
            .unwrap_or((String::new(), ColumnDataType::Varchar { length: None }));
        let edited = self.collect_column_edit(cx);
        if let Some(edited) = edited {
            let panel = self.panel.clone();
            panel.update(cx, |panel, cx| {
                if let Some(id) = config_id {
                    panel.focus_config(id);
                }
                panel.apply_column(edited, cx)
            });
        }
        // 新列名去重（旧名 + `_n`，空列名给 `column_n`）；类型沿用当前列类型
        let new_name = {
            let panel = self.panel.read(cx);
            let mut n = 2u32;
            let mut candidate = if base_name.is_empty() {
                "column_2".to_string()
            } else {
                format!("{base_name}_2")
            };
            while panel
                .draft()
                .columns
                .iter()
                .any(|c| c.def.name == candidate)
            {
                n += 1;
                candidate = if base_name.is_empty() {
                    format!("column_{n}")
                } else {
                    format!("{base_name}_{n}")
                };
            }
            candidate
        };
        let new_id = {
            let panel = self.panel.clone();
            panel.update(cx, |panel, cx| {
                if let Some(id) = config_id {
                    panel.focus_config(id);
                }
                panel.add_column(new_name.clone(), base_type, cx);
                panel
                    .draft()
                    .columns
                    .last()
                    .map(|c| c.id)
                    .unwrap_or_default()
            })
        };
        self.open_column_dialog(new_id, window, cx);
    }

    /// 表配置是否有未应用修改（取消 / ESC 丢失防护判定）。
    ///
    /// 口径：列名 / 产出类型 / 生成器配置 / 空值率 / 唯一 / 任一参数输入框的值，
    /// 任一项与打开时快照（`original*`）不同即 dirty。
    pub(crate) fn draft_is_dirty(draft: &ColumnDraft, cx: &App) -> bool {
        if draft.name_input.read(cx).value().to_string() != draft.original.name {
            return true;
        }
        if draft.def.data_type != draft.original.data_type {
            return true;
        }
        if draft.def.generator != draft.original.generator {
            return true;
        }
        if draft.null_input.read(cx).value().to_string() != draft.original_null {
            return true;
        }
        if draft.def.unique != draft.original.unique {
            return true;
        }
        for (index, p) in draft.params.iter().enumerate() {
            let current = match &p.widget {
                ParamWidget::Scalar(state) => state.read(cx).value().to_string(),
                ParamWidget::Complex(state) => state.read(cx).value().to_string(),
            };
            if current
                != draft
                    .original_params
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("")
            {
                return true;
            }
        }
        false
    }

    /// 弹出「放弃未应用的修改？」确认框（#6）：确认后清空表配置并关闭列编辑对话框。
    ///
    /// 取消按钮与 dialog `on_cancel`（ESC / 遮罩）共用。确认框叠在列编辑对话框之上
    ///（vendor 对话框支持栈式多实例）；OK 里连续两次 `close_dialog`——先关确认框自身、
    /// 再关列编辑对话框（`close_dialog_internal` 按「最后活跃」依次弹出）。
    pub(crate) fn confirm_discard(
        view: &Entity<MockDetailView>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = view.clone();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let view_for_ok = view.clone();
            alert
                .title("放弃未应用的修改？")
                .description("列编辑有未应用修改，取消后这些修改将丢失。")
                .show_cancel(true)
                .ok_text("放弃修改")
                .cancel_text("继续编辑")
                .on_ok(move |_, window, cx| {
                    view_for_ok.update(cx, |view, _cx| view.draft = None);
                    window.close_dialog(cx); // 关闭确认框自身
                    window.close_dialog(cx); // 再关闭列编辑对话框
                    true
                })
        });
    }

    /// 打开「列编辑」对话框（列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认）。
    pub fn open_column_dialog(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        // 对话框的「应用」要落回这张 tab 的配置：先把配置 id 抓住，闭包里切活动指针
        let config_id = self.config_id();
        let Some(column) = self
            .panel
            .read(cx)
            .config_for(config_id.unwrap_or_default())
            .columns
            .iter()
            .find(|c| c.id == id)
            .cloned()
        else {
            return;
        };

        let null_initial = format!("{}", (column.def.nullable_ratio * 100.0).round() as i64);
        // 第三行左栏宽度沿用上次拖拽的结果（面板字段，仍受 220–520 约束）
        let list_width = px(self.panel.read(cx).column_list_px.clamp(220., 520.));
        let null_input = cx.new(|cx| InputState::new(window, cx).placeholder("0~100"));
        null_input.update(cx, |s, cx| s.set_value(null_initial.clone(), window, cx));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("列名"));
        name_input.update(cx, |s, cx| s.set_value(column.def.name.clone(), window, cx));
        // 打开时参数序列化快照（dirty 判定：输入框值逐一与初值比较）
        let original_params = generator_catalog::spec_of(&column.def.generator)
            .params
            .iter()
            .copied()
            .map(|field| match field.kind {
                ParamKind::Complex => complex_param_text(&column.def.generator, field.key),
                _ => param_text(&column.def.generator, field.key),
            })
            .collect::<Vec<String>>();

        self.draft = Some(ColumnDraft {
            id,
            name_input,
            type_label: column_type_label(&column.def.data_type),
            def: column.def.clone(),
            original: column.def.clone(),
            original_params,
            original_null: null_initial,
            generator_edited: false,
            recommendation: None,
            _name_sub: None,
            confidence: column.confidence.clone(),
            sample_value: column.sample_value.clone(),
            params: Vec::new(),
            null_input,
            param_error: None,
            sample_preview: String::new(),
            sample_round: 0,
            param_generator: None,
            // 打开时继承这一列原来选的自定义项（内建列 / 历史重放 → None）
            picked_user: column.user_generator.clone(),
        });
        // 列名失焦 → 智能重推（#4）：若用户未主动选过生成器，且新推荐与当前生成器不同，
        // 在属性区提示「建议改用 XX（可采纳）」，供一键重推
        let name_sub = self.draft.as_ref().unwrap().name_input.clone();
        let _name_sub = cx.subscribe_in(
            &name_sub,
            window,
            move |view, emitter, ev: &InputEvent, _window, cx| {
                if !matches!(ev, InputEvent::Blur) {
                    return;
                }
                let name = emitter.read(cx).value().trim().to_string();
                let Some(draft) = view.draft.as_mut() else {
                    return;
                };
                draft.recommendation = if draft.generator_edited || name.is_empty() {
                    None
                } else {
                    let mapped = ColumnMapper::infer(&name, &draft.def.data_type);
                    if mapped.generator == draft.def.generator {
                        None
                    } else {
                        Some(mapped.generator)
                    }
                };
                cx.notify();
            },
        );
        if let Some(draft) = self.draft.as_mut() {
            draft._name_sub = Some(_name_sub);
        }
        // 参数输入行按当前生成器建立（生成器身份变化后由 `rebuild_params` 重建）
        self.rebuild_params(window, cx);
        // 初始样例预览（按当前配置生成，事件路径，不在 render 里做）
        self.refresh_sample_preview(cx);
        // 生成器库浏览区内嵌（同面板）：分组/最近/搜索/精调直接开在列编辑对话框里，
        // 选中写回表配置（`LibraryWriteback::ColumnDraft`），点「应用」才落地
        let view = cx.entity();
        let initial_generator = self.draft.as_ref().map(|draft| draft.def.generator.clone());
        self.generator_browser = Some(crate::mock_view::generator_library::create_library_state(
            self.panel.clone(),
            crate::mock_view::generator_library::LibraryWriteback {
                detail: view.downgrade(),
                column_id: id,
            },
            true,
            initial_generator,
            window,
            cx,
        ));

        let view = cx.entity();
        let panel = self.panel.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let theme = cx.theme().clone();
            let Some(draft) = view.read(cx).draft.as_ref() else {
                return dialog.title("列编辑");
            };
            let name_state = draft.name_input.clone();
            let null_state = draft.null_input.clone();
            let type_label = draft.type_label.clone();
            let unique = draft.def.unique;
            // 参数行：标量单行 / 集合多行，标签与提示都从目录派生（不手写清单）
            let param_rows: Vec<ParamRowView> = draft
                .params
                .iter()
                .map(|p| {
                    let label = generator_catalog::spec_of(&draft.def.generator)
                        .params
                        .iter()
                        .find(|f| f.key == p.key)
                        .map(|f| f.label.to_string())
                        .unwrap_or_else(|| p.key.clone());
                    match &p.widget {
                        ParamWidget::Scalar(state) => ParamRowView::Scalar {
                            label,
                            state: state.clone(),
                        },
                        ParamWidget::Complex(state) => ParamRowView::Complex {
                            label,
                            hint: complex_param_hint(&p.key),
                            state: state.clone(),
                        },
                    }
                })
                .collect();
            let param_error = draft.param_error.clone();
            // 属性区只读展示（右栏「上属性」）：当前生成器名 + 其业务组（随左栏选中即时更新）。
            // 用户点的是**自定义项**时显示自定义名（`def.generator` 只有解包后的 base，
            // 直接显示 base 名会让人以为「点的没生效」）。业务组同理按自定义项自身挂的组。
            let (generator_label, category_label) = {
                let picked_ug = draft.picked_user().and_then(|id| {
                    let panel = view.read(cx).panel.clone();
                    let panel_ref = panel.read(cx);
                    panel_ref
                        .user_generators()
                        .iter()
                        .find(|ug| ug.id == id)
                        .cloned()
                });
                let spec = generator_catalog::spec_of(&draft.def.generator);
                match picked_ug {
                    Some(ug) => (
                        format!("{}（自定义）", ug.name),
                        ug.category
                            .map(|category| category.label().to_string())
                            .unwrap_or_else(|| spec.category.label().to_string()),
                    ),
                    None => (spec.label.to_string(), spec.category.label().to_string()),
                }
            };
            // 样例预览克隆成值：draft 借用要在左右分栏渲染（需要 `&mut cx`）前结束
            let sample_preview = draft.sample_preview.clone();
            // 左栏生成器库实体（顶部搜索 + 业务/数据双 Tab 列表；选中写回表配置，
            // 见 `apply_picked_generator` 的 `ColumnDraft` 分支）
            let browser = view.read(cx).generator_browser.clone();

            // —— 1-7 优化：按当前表配置计算的提示态（纯读，不写状态）——
            // #2 产出类型不匹配：生成器实际产出 ≠ 所选类型（改类型不自动换生成器）→ 参数区顶部警示
            let actual_type = crate::generators::data_type_for_generator(&draft.def.generator);
            let type_mismatch = actual_type != draft.def.data_type;
            let actual_type_label = column_type_label(&actual_type);
            // 产出类型下拉闭包会 move `type_label`，警示行这里先克隆一份
            let type_label_for_warning = type_label.clone();
            // #4 列名失焦推荐（失焦时 `_name_sub` 订阅回填）：采纳按钮 = 按当前名 + 类型重推
            //（与「恢复智能默认」同口径），采纳后视为用户主动选定生成器，不再弹推荐
            let recommendation = draft.recommendation.clone();
            let recommendation_label = recommendation.as_ref().and_then(|cfg| {
                generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .find(|s| generator_catalog::default_of(s.name) == Some(cfg.clone()))
                    .map(|s| s.label.to_string())
            });
            // #5 零参生成器（如 UUID v4 / 布尔 / 时长）：参数区标题下显示空态文案
            let zero_param = generator_catalog::spec_of(&draft.def.generator).params.is_empty();

            // 右栏（用户定稿「上属性、下参数」）：属性区 = 生成器名 / 产出类型 / 业务组
            //（生成器身份只读展示；产出类型可改——改类型不自动换生成器，按新类型重推要点
            // 上方行 2 的「智能推荐」）；
            // 参数区 = 参数行 / 样例 / 空值率 / 唯一值。列名已移到窗口顶部。
            let mut right = div().v_flex().gap_2().w_full();
            // —— 属性 ——（分区标题与右 Dock 的三处段标题同一支原语：`section` 自带下缘线）
            right = right.child(section(&theme, "属性"));
            // 生成器名（只读）：点左栏列表换生成器（选中即写回表配置，属性 / 参数随之重建）
            right = right.child(kv(
                &theme,
                "生成器名",
                div()
                    .id("mock-col-generator")
                    .test_support()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(generator_label),
            ));
            // #4 列名失焦智能重推：提示已上移到行 2 的「智能推荐」块（与「历史生成器」并排），
            // 右栏属性区只留只读的生成器身份——同一提示不两处出现。
            // 产出类型（可改，下拉）
            right = right.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("产出类型"),
                    )
                    .child({
                        let view = view.clone();
                        dropdown_button("mock-col-type", type_label.clone())
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut menu = menu;
                                for candidate in COLUMN_TYPES {
                                    let label = column_type_label(&candidate);
                                    let is_current = label == type_label;
                                    let view = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label).checked(is_current).on_click(
                                            move |_, _, app| {
                                                let chosen = candidate.clone();
                                                view.update(app, |view, cx| {
                                                    if let Some(draft) = view.draft.as_mut() {
                                                        draft.type_label =
                                                            column_type_label(&chosen);
                                                        draft.def.data_type = chosen;
                                                        draft.confidence = "manual".to_string();
                                                    }
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                menu
                            })
                    }),
            );
            // 业务组（只读）：生成器所属业务分类（左栏业务视角的组名）
            right = right.child(kv(
                &theme,
                "业务组",
                div()
                    .id("mock-col-category")
                    .test_support()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(category_label),
            ));
            // 类型与生成器解耦的说明：改类型不自动换生成器（避免参数行与生成器不一致）；
            // 想按新类型重推生成器 → 点上方「智能推荐」（行 2 左块）
            right = right.child(hint_line(
                &theme,
                "改类型不会自动换生成器；按新类型重推请点上方「智能推荐」。",
            ));
            // #4 列名失焦重推：提示摆在属性区（行 2 只有一个「智能推荐」按钮，提示放那里
            // 会把行 2 撑成两行）。点了即采纳，采纳后不再弹。
            if let (Some(cfg), Some(label)) = (&recommendation, &recommendation_label) {
                let view_adopt = view.clone();
                let cfg_for_click = cfg.clone();
                let label = label.clone();
                right = right.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child(format!("建议改用「{label}」")),
                        )
                        .child(
                            button("mock-col-adopt", "采纳", ActionKind::Quiet)
                                .xsmall()
                                .on_click(move |_, window, app| {
                                    view_adopt.update(app, |view, cx| {
                                        let Some(draft) = view.draft.as_mut() else {
                                            return;
                                        };
                                        draft.generator_edited = true;
                                        draft.recommendation = None;
                                        // 写回表配置：采纳推荐配置（列名已失焦重推，
                                        // 配置按当前名 + 类型）
                                        let mapped = ColumnMapper::infer(
                                            &draft.name_input.read(cx).value(),
                                            &draft.def.data_type,
                                        );
                                        draft.def.generator = cfg_for_click.clone();
                                        draft.confidence = mapped.confidence;
                                        draft.sample_value = mapped.sample_value;
                                        cx.defer_in(window, |view, window, cx| {
                                            view.rebuild_params(window, cx);
                                        });
                                        cx.notify();
                                    });
                                }),
                        ),
                );
            }
            // —— 参数 ——
            right = right.child(section(&theme, "参数"));
            // #2 产出类型不匹配警示：生成器实际产出与所选类型不一致时，参数区顶部提示
            //（改类型不自动换生成器是设计，这里把「不一致」讲明，指向更换生成器或恢复默认）
            if type_mismatch {
                right = right.child(
                    warning_line(
                        &theme,
                        format!(
                            "当前生成器产出 {actual_type_label}，与所选 {type_label_for_warning} 不匹配——请更换生成器或点上方「智能推荐」。"
                        ),
                    )
                    .id("mock-col-type-mismatch")
                    .test_support(),
                );
            }
            // #5 零参生成器：参数区没有表单行时显示空态文案（样例/空值率/唯一仍可用）
            if param_rows.is_empty() && zero_param {
                right = right.child(
                    hint_line(&theme, "该生成器无需参数。")
                        .id("mock-col-params-empty")
                        .test_support(),
                );
            }
            for row in param_rows {
                right = right.child(match row {
                    ParamRowView::Scalar { label, state } => form_line(&theme, &label, &state),
                    ParamRowView::Complex { label, hint, state } => {
                        complex_form_line(&theme, &label, hint, &state)
                    }
                });
            }
            if let Some((_, message)) = param_error {
                right = right.child(error_line(&theme, message));
            }
            // 参数实时样例预览（2 条示意值）：输入参数即改即看，固定种子可复现；
            // 也可以点「重新生成」手动重抽（用户要求样例自带生成按钮）
            right = right.child(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("样例"),
                            )
                            .child({
                                let view = view.clone();
                                button(
                                    "mock-col-sample-refresh",
                                    "重新生成",
                                    ActionKind::Quiet,
                                )
                                .xsmall()
                                .tooltip("按当前参数重新抽样（固定种子，结果可复现）")
                                .tooltip_show_delay(TOOLTIP_DELAY)
                                .on_click(move |_, _window, app| {
                                    view.update(app, |view, cx| {
                                        // 批次 +1：换一批示意值（同一批次内仍然可复现）
                                        if let Some(draft) = view.draft.as_mut() {
                                            draft.bump_sample_round();
                                        }
                                        view.refresh_sample_preview(cx);
                                        cx.notify();
                                    });
                                })
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground)
                            .child(sample_preview),
                    ),
            );
            right = right.child(form_line(&theme, "空值率（%）", &null_state));
            right = right.child(kv(
                &theme,
                "唯一值",
                Switch::new("mock-col-unique").checked(unique).on_click({
                    let view = view.clone();
                    move |checked, _window, app| {
                        let checked = *checked;
                        view.update(app, |view, cx| {
                            if let Some(draft) = view.draft.as_mut() {
                                draft.def.unique = checked;
                            }
                            cx.notify();
                        });
                    }
                }),
            ));

            // 窗口主体：三横行（用户定稿）——行 1「列名 + 搜索定位」、行 2「智能推荐 | 历史
            // 生成器」、行 3「左：业务分类/数据类型 + 右：属性·参数·样例」。整块定高
            // （`COLUMN_DIALOG_HEIGHT`），内容超高在各块内部滚动，不再由内容把对话框撑高；
            // 行 3 左右可拖（左列宽可调），右列永远跟左列选中的生成器走——左选右变，
            // 点「应用」才落地。
            // 分栏拖拽回写用的面板句柄：必须在每帧这里 clone（`open_dialog` 的 builder 是
            // `Fn`，外面捕获的 Entity 不能再 move 进内层 `'static` 回调）
            let col_split_panel = panel.clone();
            let body = div()
                .v_flex()
                .gap_2()
                .w_full()
                .h(ui::COLUMN_DIALOG_HEIGHT)
                .min_h_0()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .w_full()
                        // 列名（定宽）：本列名字，与搜索定位框同排
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .flex_none()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(text_hint(&theme))
                                        .child("列名"),
                                )
                                .child(
                                    div()
                                        .id("mock-col-name")
                                        .test_support()
                                        .w(ui::COLUMN_NAME_INPUT_WIDTH)
                                        .child(Input::new(&name_state)),
                                ),                        )
                        // 搜索定位框（跨剩余宽度，过滤左栏分组项）
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when_some(browser.clone(), |col, b| {
                                    col.child(
                                        crate::mock_view::generator_library::render_search_box(
                                            &b, window, cx,
                                        ),
                                    )
                                }),
                        ),
                )
                // 顶部行与主体之间一条更轻的线：上半是「这一列叫什么」，
                // 下半是「它用什么造」，两块不是一回事
                .child(card_divider(&theme))
                // —— 行 2：智能推荐（左，一个按钮） | 历史生成器（右，一行 chips） ——
                // 左边是「按这一列的名字和类型该用什么」（推导），右边是「这个会话里已经用过
                // 什么」（回访）。两块都压成**单行**：行 2 高度必须可预测，否则会把行 3 的
                // 库挤矮；chips 不换行（见 `render_history_chips`）。
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .flex_none()
                        .child(if let Some(b) = browser.as_ref() {
                            crate::mock_view::generator_library::render_smart_button(
                                b, &theme, window, cx,
                            )
                        } else {
                            div().into_any_element()
                        })
                        .child(Separator::vertical().color(theme.colors.border))
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("历史"),
                        )
                        .child(if let Some(b) = browser.as_ref() {
                            crate::mock_view::generator_library::render_history_chips(
                                b, &theme, window, cx,
                            )
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(card_divider(&theme))
                // —— 行 3：左 = 库（业务分类下的生成器 / 数据类型下的方法），
                //           右 = 这一列的属性 · 参数 · 样例（随左列选中即时重建） ——
                // 左右可拖（左列宽 220~520，右列吃剩余），两栏各自内部滚动：展开分组、
                // 切 Tab 只让库那一栏滚，不会把对话框拉高
                .child(
                    div().flex_1().min_h_0().w_full().child(
                        h_resizable("mock-col-split")
                            .child(
                                resizable_panel()
                                    .size(list_width)
                                    .size_range(px(220.)..px(520.))
                                    .child(
                                        div()
                                            .size_full()
                                            .min_h_0()
                                            .p_2()
                                            .overflow_y_scrollbar()
                                            .when_some(browser, |col, b| {
                                                col.child(
                                                    crate::mock_view::generator_library::render_browse_pane(
                                                        &b, &theme, window, cx,
                                                    ),
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                resizable_panel().flex_basis(px(0.)).child(
                                    div()
                                        .size_full()
                                        .min_h_0()
                                        .p_2()
                                        .overflow_y_scrollbar()
                                        .child(right),
                                ),
                            )
                            // 拖到哪记到哪：左栏宽度存面板字段，重开对话框沿用（不是「关掉就回 280」）
                            .on_resize(move |state, _window, app| {
                                if let Some(width) = state.read(app).sizes().first().copied() {
                                    col_split_panel.update(app, |panel, cx| {
                                        panel.column_list_px = f32::from(width).clamp(220., 520.);
                                        cx.notify();
                                    });
                                }
                            })
                    )
                );

            let view_apply = view.clone();
            let view_cancel = view.clone();
            let view_cancel_esc = view.clone();
            let view_next = view.clone();
            let panel_apply = panel.clone();
            dialog
                .title("列编辑")
                .width(ui::COLUMN_DIALOG_WIDTH)
                .child(body)
                // #6 ESC / 遮罩关闭拦截：有未应用修改 → 弹确认框并阻止本次关闭
                .on_cancel(move |_event, window, app| {
                    let dirty = view_cancel_esc
                        .read(app)
                        .draft
                        .as_ref()
                        .map(|d| Self::draft_is_dirty(d, app))
                        .unwrap_or(false);
                    if dirty {
                        Self::confirm_discard(&view_cancel_esc, window, app);
                        false
                    } else {
                        true
                    }
                })
                .footer(
                DialogFooter::new()
                    .child(
                        button("mock-col-cancel", "取消", ActionKind::Soft)
                            .on_click(move |_, window, app| {
                                let dirty = view_cancel
                                    .read(app)
                                    .draft
                                    .as_ref()
                                    .map(|d| Self::draft_is_dirty(d, app))
                                    .unwrap_or(false);
                                if dirty {
                                    // 有未应用修改：弹确认框，确认后才真正清表配置并关闭
                                    Self::confirm_discard(&view_cancel, window, app);
                                } else {
                                    view_cancel.update(app, |view, _cx| view.draft = None);
                                    window.close_dialog(app);
                                }
                            }),
                    )
                    .child(
                        button("mock-col-apply", "应用", ActionKind::Main)
                            .on_click(move |_, window, app| {
                                let edited =
                                    view_apply.update(app, |view, cx| view.collect_column_edit(cx));
                                if let Some(edited) = edited {
                                    panel_apply.update(app, |panel, cx| {
                                        if let Some(id) = config_id {
                                            panel.focus_config(id);
                                        }
                                        panel.apply_column(edited, cx)
                                    });
                                }
                                window.close_dialog(app);
                            }),
                    )
                    // #7 应用并新增下一列：应用当前表配置 → 关对话框 → 按当前列名 +2 去重新建列
                    //（沿用 §8.3 字段预设命名策略）→ 立即打开新列的列编辑对话框（连续录入）
                    .child(
                        button(
                            "mock-col-apply-next",
                            "应用并新增下一列",
                            ActionKind::Soft,
                        )
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                                view_next.update(app, |view, cx| view.apply_and_next(window, cx));
                            }),
                    ),
            )
        });
    }

    /// 按**当前列名**（输入框里的值）+ 类型重推这一列的生成器配置（左栏「智能推荐」）。
    ///
    /// 与右栏的「采纳推荐」（只换生成器、保留参数）不同：这里整套重来
    /// （生成器 + 参数 + 空值率 + 唯一值都回到推荐值）——它是「让这一列回到推断结果」。
    pub(crate) fn smart_reset(
        &mut self,
        column_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        if draft.id != column_id {
            return;
        }
        // 按输入框当前值（用户可能刚改过列名还没应用），不按打开对话框时的快照
        let name = draft.name_input.read(cx).value().to_string();
        let mapped = ColumnMapper::infer(&name, &draft.def.data_type);
        draft.def.generator = mapped.generator;
        draft.def.nullable_ratio = 0.0;
        draft.def.unique = false;
        draft.confidence = mapped.confidence;
        draft.sample_value = mapped.sample_value;
        // 智能推荐 = 用户主动选定生成器，之后列名失焦不再弹推荐；
        // 它给的是内建推断结果，所以自定义项身份一并清掉（否则左栏还亮着原来那一项）
        draft.generator_edited = true;
        draft.picked_user = None;
        draft.recommendation = None;
        draft.param_error = None;
        // 生成器身份可能变了：参数行按新生成器重建（事件路径，不能在 render 做）
        cx.defer_in(window, |view, window, cx| {
            view.rebuild_params(window, cx);
        });
        cx.notify();
    }

    /// 按当前生成器重建参数输入行（保留同名参数的输入框与订阅）。
    /// 列编辑对话框内选了生成器（库对话框写回表配置，`LibraryWriteback::ColumnDraft`）：
    /// 写 draft 的生成器配置并重建参数区 / 样例预览。列还没应用，面板列不动——
    /// 与应用按钮同口径（改动留在表配置，点「应用」才落地）。
    pub(crate) fn apply_generator_config_to_draft(
        &mut self,
        column_id: u64,
        config: GeneratorConfig,
        picked_user: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        if draft.id != column_id {
            return;
        }
        draft.def.generator = config;
        // 「点的是哪一项」：内建 = None（按 base 名认领即可），自定义 = 那一项的 id
        // （否则同一 base 的多个自定义项在左栏会一起高亮、右栏只能显示 base 名）
        draft.picked_user = picked_user;
        draft.confidence = "manual".to_string();
        // #4：左栏选中 = 用户主动选定生成器，之后列名失焦不再弹推荐
        draft.generator_edited = true;
        draft.recommendation = None;
        draft.param_error = None;
        self.rebuild_params(window, cx);
        self.refresh_sample_preview(cx);
        cx.notify();
    }

    fn rebuild_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let generator = draft.def.generator.clone();
        let spec = generator_catalog::spec_of(&generator);
        let fields: Vec<ParamField> = spec.params.iter().copied().collect();
        // 换生成器时旧参数**一律不沿用**：`length` / `min` 这类同名字段在新生成器下含义
        // 可能完全不同（甚至越界），沿用会让人误以为「参数没切换成功」。
        // 只有同一生成器重建（改列名重推 / 参数校验失败重画）才复用输入框。
        let same_generator = draft.param_generator.as_deref() == Some(spec.name);
        draft.param_generator = Some(spec.name.to_string());
        let mut previous: HashMap<String, ParamInput> = if same_generator {
            draft.params.drain(..).map(|p| (p.key.clone(), p)).collect()
        } else {
            draft.params.clear();
            HashMap::new()
        };
        let mut next = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(item) = previous.remove(field.key) {
                next.push(item);
                continue;
            }
            let key = field.key.to_string();
            let (widget, _sub) = match field.kind {
                // 集合 / 加权选项：多行文本，改一次解析一次（解析失败保留上一个有效值）
                ParamKind::Complex => {
                    let state = cx.new(|cx| TextareaState::new(window, cx));
                    let initial = complex_param_text(&generator, field.key);
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            view.commit_complex_param(&key_for_closure, &text, cx);
                            // 参数变化 → 样例预览同步刷新（commit 已写回生成器或记下解析错误）
                            view.refresh_sample_preview(cx);
                        },
                    );
                    (ParamWidget::Complex(state), _sub)
                }
                kind => {
                    let initial = param_text(&generator, field.key);
                    let state = cx.new(|cx| InputState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            let mut changed = false;
                            if let Some(draft) = view.draft.as_mut() {
                                if let Some(patched) =
                                    patch_param(&draft.def.generator, &key_for_closure, &text, kind)
                                {
                                    draft.def.generator = patched;
                                    draft.confidence = "manual".to_string();
                                    draft.sample_value.clear();
                                    changed = true;
                                }
                            }
                            if changed {
                                // 参数变化 → 样例预览同步刷新（事件路径，不在 render 做）
                                view.refresh_sample_preview(cx);
                                cx.notify();
                            }
                        },
                    );
                    (ParamWidget::Scalar(state), _sub)
                }
            };
            next.push(ParamInput { key, widget, _sub });
        }
        if let Some(draft) = self.draft.as_mut() {
            draft.params = next;
            // 参数行重建 = 生成器身份变了（换生成器 / 恢复智能默认）：旧参数的报错不再适用
            draft.param_error = None;
        }
        cx.notify();
    }

    /// 复杂参数（集合 / 加权选项）编辑：解析成功就写回生成器；失败则**保留上一个有效值**
    /// 并记下原因（只在对话框里就地提示，不把无效内容写进生成器）。
    pub(crate) fn commit_complex_param(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        match parse_complex_param(key, text) {
            Ok(value) => {
                if let Some(patched) = patch_param_value(&draft.def.generator, key, value) {
                    draft.def.generator = patched;
                    draft.confidence = "manual".to_string();
                    draft.sample_value.clear();
                }
                draft.param_error = None;
            }
            Err(reason) => draft.param_error = Some((key.to_string(), reason)),
        }
        cx.notify();
    }

    /// 按**当前输入框值**生成 2 条样例（参数实时预览）。
    ///
    /// 参数订阅里每次 Change 都会调它（见 `rebuild_params`）：生成器配置已由
    /// `patch_param` 实时写回，所以这里直接读 `draft.def.generator` 即可。
    /// 固定种子保证样例稳定可读（与最终生成的随机种子无关，纯示意）；
    /// 点「重新生成」时批次 +1，换一批（每批内部仍稳定）。
    pub(crate) fn refresh_sample_preview(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let locale = self.panel.read(cx).draft().options.locale.clone();
        // 固定种子保证「同一批参数看到的样例不变」（输入参数不会让示例乱跳）；
        // 点「重新生成」时批次 +1 → 换一批。种子按批次平移，每批都仍可复现
        let mut rng = StdRng::seed_from_u64(SAMPLE_PREVIEW_SEED.wrapping_add(draft.sample_round));
        // 生成器对「空参数区间」（如 min==max）会由 fake crate 断言 panic：
        // 预览是示意，不允许把异常带进事件路径，`catch_unwind` 兜底成占位文案
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let samples: Vec<String> = (0..2)
            .map(|row| {
                catch_unwind(AssertUnwindSafe(|| {
                    crate::generators::generate_cell(&draft.def.generator, &mut rng, row, &locale)
                }))
                .unwrap_or_else(|_| "（参数区间为空，无法采样）".to_string())
            })
            .collect();
        draft.sample_preview = samples.join("，");
    }

    /// 字段清单（列 + 生成器 + 参数摘要 + 编辑 / 智能 / 删除）。
    /// 字段区：当前搜索词下要画的列（保序；空词 = 全部）。搜索词归面板（单一权威）。
    ///
    /// 匹配规则在 [`column_matches`]（列名或生成器标签，子串、大小写不敏感）：
    /// 「搜索词一变就重画」的订阅在面板那边（`ensure_inputs`）。
    pub(crate) fn visible_columns(
        &self,
        panel: &MockPanel,
        id: u64,
        cx: &App,
    ) -> Vec<MockColumnSpec> {
        let query = panel.column_filter_query(id, cx);
        panel
            .config_for(id)
            .columns
            .iter()
            .filter(|column| {
                column_matches(
                    &query,
                    &column.def.name,
                    generator_catalog::spec_of(&column.def.generator).label,
                    &column_type_label(&column.def.data_type),
                    &column.confidence,
                )
            })
            .cloned()
            .collect()
    }

    fn render_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // 每张 tab 的列操作都落回它自己那份配置
        let config_id = self.config_id();
        // 卡片 / chip / 按钮的形态都从 `style` 原语取，这里只留卡片说明行的两色
        let theme = cx.theme().clone();
        let muted = theme.colors.muted_foreground;
        let info = theme.colors.info;

        // 先看「这张表有没有列」再套筛选：空表与「筛没了」是两句不同的话
        let has_any = {
            let id = self.config_id();
            let panel = self.panel.read(cx);
            id.map(|id| !panel.config_for(id).columns.is_empty())
                .unwrap_or(false)
        };
        if !has_any {
            return div()
                .text_xs()
                .text_color(muted)
                .child("暂无列：在这张 tab 里点「导入结构」或「＋ 加列」开始造表。")
                .into_any_element();
        }
        let columns = {
            let panel = self.panel.read(cx);
            self.visible_columns(panel, config_id.unwrap_or_default(), cx)
        };
        if columns.is_empty() {
            return div()
                .text_xs()
                .text_color(muted)
                .child("没有匹配的列——清空「筛选列」看全部（匹配列名与生成器名）。")
                .into_any_element();
        }

        // 正在编辑哪一列（列编辑对话框开着时才有）：那张卡给左缘强调条。
        // 对话框会盖住卡片区，关掉之后要能一眼看出「刚才改的是这一列」——
        // 尤其列多了要往下滚，没有这条标识就得靠记名字去找。
        let editing_id = self.draft.as_ref().map(|draft| draft.column_id());

        // 单行卡片之间 4px 就够（8px 会让 20 列多出 80px 纯空白）
        let mut list = div().v_flex().gap_1().w_full();
        // enumerate 拿序号：首 / 末列禁用对应的 ↑/↓（列序 = 生成顺序）
        for (index, column) in columns.iter().enumerate() {
            let spec = generator_catalog::spec_of(&column.def.generator);
            let summary = summarize_params(&column.def.generator);
            let id = column.id;
            // 自定义项显示**用户起的名字**（同一 base 的内建项另有名字，显示 base 名会认错）
            let label = column
                .user_generator
                .as_ref()
                .and_then(|user_id| {
                    self.panel
                        .read(cx)
                        .user_generators()
                        .iter()
                        .find(|ug| ug.id == *user_id)
                        .map(|ug| ug.name.clone())
                })
                .unwrap_or_else(|| spec.label.to_string());
            let unique = if column.def.unique { "唯一 · " } else { "" };
            let null_percent = (column.def.nullable_ratio * 100.0).round() as i64;
            let detail = if summary.is_empty() {
                column.sample_value.clone()
            } else {
                summary.clone()
            };
            // S1④：跨表引用徽标 + 常驻说明（挂在列上的关系，不是外键 DDL）
            let ref_badge = column
                .def
                .dependency
                .as_ref()
                .map(|dep| format!("↳ 引用 {}.{}", dep.ref_table, dep.ref_column));
            let ref_note =
                column.def.dependency.is_some().then(
                    || "引用目标仅支持自增主键，按自增规律推算取值域，不读取父表已生成的数据。",
                );
            let panel = self.panel.clone();
            let entity = cx.entity();
            let is_first = index == 0;
            let is_last = index + 1 == columns.len();
            // 卡片焦点句柄：卡片要能被 Tab 到（四个操作是悬停才显形的，键盘用户没有悬停，
            // 焦点落在卡片上时也必须显形）。按业务键（列 id）缓存句柄——每帧新建会让
            // 焦点在重绘时丢（`use_keyed_state` 是按元素 id 缓存的）。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!("mock-field-card-{id}"))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                // 克隆成自有值：`read(cx)` 返回的引用会一路借住 `cx`，
                // 而下面 `card_border` 的 `transition(..)` 要可变借用 `cx`
                .clone();
            let card_focused = card_handle.is_focused(window);
            let active = self.hovered_column == Some(id) || card_focused;

            // ── 单行：列名 · 类型 · 生成器 · 参数摘要 · 操作（常驻）──
            // A 方案的落点：两行卡片 20 列要 1300px+，一屏只看得到 5 列；压成 34px 一行后，
            // 同样高度能看 3 倍，且四样信息落在同一基线上，竖着扫就能比（不上升成表格）。
            // 操作常驻而不是悬停才冒出来：不可发现的按钮等于不存在，用户只会用「删除」。
            let summary_text = format!("{unique}空值 {null_percent}% · {detail}");
            // 悬停全文 = 生成器名 + 参数全文；引用列的「为什么不读父表数据」并进来
            // （单行放不下一整行解释，但这句话必须仍可查）
            let full_text = match ref_note {
                Some(note) if summary.is_empty() => format!("生成器：{label}\n\n{note}"),
                Some(note) => format!("生成器：{label}\n{summary}\n\n{note}"),
                None if summary.is_empty() => format!("生成器：{label}"),
                None => format!("生成器：{label}\n{summary}"),
            };
            let is_reference = ref_badge.is_some();

            // 身份区：列名定宽（四段对齐的起点）+ 类型 + 置信度 + 引用信号
            let mut identity = div()
                .h_flex()
                .items_center()
                .gap_2()
                .flex_none()
                .w(rems(ui::COLUMN_NAME_CELL_WIDTH))
                .child(
                    card_title(column.def.name.clone())
                        .max_w(rems(ui::COLUMN_NAME_CELL_WIDTH))
                        .text_ellipsis(),
                )
                .child(data_type_tag(&theme, &column.def.data_type));
            // high 是默认档：每列都挂一个绿色 high，只会把「哪列可信度低」淹掉
            if column.confidence != "high" {
                identity = identity.child(confidence_tag(&theme, &column.confidence));
            }
            // 引用列只留一个 ↳：完整「引用 表.列」在悬停全文里，行内宽度留给参数摘要
            if is_reference {
                identity = identity.child(div().flex_none().text_xs().text_color(info).child("↳"));
            }

            // 操作区常驻（四个都有 tooltip；ghost 图标不抢卡片内容的视觉重量）
            let ops = div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-moveup-{id}"),
                        IconName::ArrowUp,
                        "上移（列序即生成顺序）",
                    )
                    .disabled(is_first)
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.move_column(id, true, cx)
                        });
                    })
                })
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-movedown-{id}"),
                        IconName::ArrowDown,
                        "下移（列序即生成顺序）",
                    )
                    .disabled(is_last)
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.move_column(id, false, cx)
                        });
                    })
                })
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-copy-{id}"),
                        IconName::Copy,
                        "复制列（快速造相似字段）",
                    )
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.duplicate_column(id, cx)
                        });
                    })
                })
                .child(
                    danger_icon_button(
                        format!("mock-del-{id}"),
                        IconName::Delete,
                        "删除列",
                        &theme,
                    )
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.remove_column(id, cx)
                        });
                    }),
                );

            let summary_el = HoverCard::new(ElementId::Name(SharedString::from(format!(
                "mock-hover-{id}"
            ))))
            .open_delay(std::time::Duration::from_millis(300))
            .trigger_style(StyleRefinement::default().flex_1().min_w_0())
            .trigger(card_note(&theme, summary_text))
            .content(move |_, _, cx| {
                // content 是 `Fn`（可多次调用）：每次克隆文本，不消耗捕获值
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(full_text.clone())
            });

            // 描边色带过渡：悬停 / 聚焦 → `ring`，离开 → `border`（`card_border` 内部插值）。
            // 走 `card`（不带 CSS `.hover()`）而不是 `card_interactive`：CSS hover 是即时切换，
            // 插不了值。悬停状态视图自己记着（`hovered_column`）。
            let border = card_border(
                &theme,
                active,
                format!("mock-field-border-{id}"),
                window,
                cx,
            );
            // 卡片本身仍可 Tab 到：操作虽已常驻，键盘用户要靠焦点知道停在哪一列
            let mut row = card(&theme)
                .id(ElementId::Name(SharedString::from(format!(
                    "mock-field-{id}"
                ))))
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .min_h(ui::COLUMN_ROW_HEIGHT)
                .border_color(border)
                // 强调条是绝对定位的：卡片必须是定位上下文
                .relative()
                .track_focus(&card_handle)
                .tab_stop(true)
                .on_hover({
                    let entity = cx.entity();
                    move |&hovering, _, app| {
                        entity.update(app, |view, cx| {
                            let next = hovering.then_some(id);
                            if view.hovered_column != next {
                                view.hovered_column = next;
                                cx.notify();
                            }
                        });
                    }
                })
                .child(identity)
                // 生成器入口 = 编辑入口（同一个对话框：选生成器 + 调参数）
                .child(
                    div()
                        .flex_none()
                        .max_w(rems(ui::COLUMN_GENERATOR_CELL_WIDTH))
                        .overflow_hidden()
                        .child(
                            button(
                                SharedString::from(format!("mock-gen-{id}")),
                                label,
                                ActionKind::Quiet,
                            )
                            .xsmall()
                            .dropdown_caret(true)
                            .tooltip("打开列编辑：选生成器 + 调参数（同一处改完再应用）")
                            .tooltip_show_delay(TOOLTIP_DELAY)
                            .on_click(move |_, window, app| {
                                entity.update(app, |view, cx| {
                                    view.open_column_dialog(id, window, cx);
                                });
                            }),
                        ),
                )
                .child(summary_el)
                .child(ops);
            // 「正在编辑这一列」：左缘 3px 强调条，浮在卡片左缘（不挤动卡内内容）
            if editing_id == Some(id) {
                row = row.child(current_bar(&theme));
            }
            list = list.child(row);
        }

        div()
            .id("mock-field-list")
            .v_flex()
            .gap_1()
            .w_full()
            .child(list)
            .into_any_element()
    }
    /// 结果表 tab 的列清单（**只读**）：列来自这次生成（`MockGenInfo.columns`）。
    ///
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

impl Render for MockDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;
        let is_draft = matches!(self.target, DetailTarget::Config(_));
        let config_id = self.config_id();
        // 本 tab 的稳定唯一键（`DetailTarget::key`）：tab 内所有会随 tab 挂树的元素
        // id 都拼它，保证同一窗口内 id 唯一（a11y 树重复节点会 panic，见
        // `multiple_detail_tabs_do_not_collide_a11y_ids`）。
        let tab_key = self.target.key();

        // 一次读齐渲染要用的状态：读租约不能跨到 `cx` 的独占使用处，而下面的
        // `panel.update`（表头要懒创建输入、按钮要进面板）必须独占面板。
        let (
            table,
            generated,
            scenario_source,
            relation_note,
            draft_columns,
            draft_columns_shown,
            result_columns,
            dropped_by_project_switch,
            preview_sort_label,
            preview_shown,
        ) = {
            let panel = self.panel.read(cx);
            // 本 tab 看哪张表：表配置 tab 看表配置目标表，结果表 tab 看它自己
            // （与面板的「当前表」无关——切 tab 就能对照两张表的预览）
            let table = match &self.target {
                DetailTarget::Config(id) => panel.config_for(*id).table_name.clone(),
                DetailTarget::Table(name) => name.clone(),
            };
            let generated = panel
                .results()
                .iter()
                .find(|info| info.table_name == table)
                .cloned();
            let relation_note = if is_draft {
                None
            } else {
                panel.relation_note_for(&table)
            };
            let result_columns = generated
                .as_ref()
                .map(|info| info.columns.len())
                .unwrap_or(0);
            // 预览当前显示的那一份（排序生效时是重查来的），标题里的行数与排序说明都从它来
            let shown = panel.preview_for(&table);
            (
                table,
                generated,
                panel.scenario_source().map(|s| s.to_string()),
                relation_note,
                config_id
                    .map(|id| panel.config_for(id).columns.len())
                    .unwrap_or(0),
                // 字段区的筛选计数与渲染读同一处（`visible_columns`），不另写一份口径
                config_id
                    .map(|id| self.visible_columns(panel, id, cx).len())
                    .unwrap_or(0),
                result_columns,
                panel.results_dropped_by_project_switch(),
                shown.and_then(|(_, sort)| sort).map(|sort| sort.label()),
                shown.map_or(0, |(preview, _)| preview.rows.len()),
            )
        };

        // ── 表头 ──
        // 表配置 tab：这张表的设计与生成状态（表名 / 行数 / 种子 / 语言 + 生成 + 进度 + 失败重试）。
        // 状态归面板（单一权威），画在中央——它属于这张表，不属于「有哪些表」那份清单。
        // 结果表 tab：只读事实 + 跨表后果；不给「生成」，免得像能只重跑这一张。
        let head = if let (true, Some(id)) = (is_draft, config_id) {
            self.panel.update(cx, |panel, cx| {
                // 传配置 id + tab 键：输入框按配置各自一份，出口按钮 id 按 tab 唯一
                panel.render_design_head(id, &tab_key, window, cx)
            })
        } else {
            // 事实行：主行只留语义事实（表名 / 行数 / 状态）；临时表名与耗时是技术细节，
            // 收进悬停（低频解释不常驻，与右面板状态条同一手法）。
            //
            // 没结果分两种：切项目把结果清掉了（旧 tab 留着，见 D35），或者这一轮没生成它。
            // 两种说法的行动建议一样（重新生成），但「为什么没结果」不一样——不能都说成「这一轮」。
            let facts_row = match generated.as_ref() {
                Some(info) => div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .text_ellipsis()
                            .child(format!(
                                "{} · {} 行 · 已生成",
                                table,
                                with_thousands(u64::from(info.row_count)),
                            )),
                    )
                    .child({
                        let temp_table = info.temp_table_name.clone();
                        let elapsed_ms = info.elapsed_ms;
                        HoverCard::new(ElementId::Name(SharedString::from(format!(
                            "mock-facts-tech-{tab_key}"
                        ))))
                        .trigger(info_hint_icon(&theme))
                        .content(move |_, _, _cx| {
                            div()
                                .p_2()
                                .text_xs()
                                .child(format!(
                                    "临时表 {temp_table} · 生成耗时 {elapsed_ms} ms"
                                ))
                        })
                    }),
                None if dropped_by_project_switch => div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(format!(
                        "{table} · 所属项目已切换：这张表的结果已作废（切项目会清空 mock 结果）——重新生成后再看"
                    )),
                None => div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(format!("{table} · 这一轮没有它的结果（重新生成后再看）")),
            };
            let mut head = div().v_flex().gap_1().w_full().child(facts_row);
            // 场景来源：常驻只说「从哪来」（出处是每次都要读的身份信息）；
            // 「怎么切到别的表」是一次性引导，收进悬停。
            if let Some(source) = scenario_source.clone() {
                head = head.child(
                    HoverCard::new(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-source-{tab_key}"
                    ))))
                    .trigger(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(card_note(&theme, format!("来自场景模板「{source}」")))
                            .child(info_hint_icon(&theme)),
                    )
                    .content(move |_, _, _cx| {
                        div().p_2().text_xs().child(
                            "列与预览都属于它自己；看别的表切 tab（或在右 Dock 结果表清单点一行）",
                        )
                    }),
                );
            }
            if let Some(note) = relation_note.clone() {
                head = head.child(warning_line(&theme, note));
            }
            head
        };

        // 段 1：基本信息（表头）。内容可能超出一屏（进度行 / 失败重试 / 来源说明），
        // 进可拖分栏后有自己的滚动，不再把整页拖长。
        let head_block = div()
            .v_flex()
            .gap_1()
            .w_full()
            .pb_2()
            .border_b_1()
            .border_color(border)
            .child(head);

        let mut body = div()
            .id(format!("mock-detail-{tab_key}"))
            .role(gpui::Role::Group)
            .v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .p_3();

        // ── 列（段 2）──
        // 三种态各自成块：无列一行提示 / 列操作行 + 单行列清单 / 结果表只读列。
        // **不画「列（N）」段标题**：那一行每份配置固定吃掉一段高度，换来的只是一个计数；
        // 计数并到操作行最左，跟着按钮一起读。
        let column_count = if draft_columns_shown == draft_columns {
            format!("{draft_columns} 列")
        } else {
            format!("{draft_columns_shown} / {draft_columns} 列 · 已筛选")
        };
        let mut columns_block = div().v_flex().gap_2().w_full();
        if is_draft && draft_columns == 0 {
            // 无列：一行提示 + 起手按钮，不套卡片、不给标题——空态的职责是「告诉你下一步」，
            // 不是占掉列区高度（列区高度按剩余空间分配，撑一块卡就把预览段压下去）
            columns_block = columns_block.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(card_note(&theme, "还没有列"))
                    .child(self.panel.update(cx, |panel, cx| {
                        panel.render_scenario_button("mock-detail-scenario-empty", cx)
                    }))
                    .child(self.panel.update(cx, |panel, cx| {
                        panel.render_column_source_buttons(config_id.unwrap_or_default(), cx)
                    })),
            );
        } else if is_draft {
            columns_block = columns_block
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        // 计数在操作行最左：它和「场景 / 筛选 / 加列」是同一句话（这份清单现在多大）
                        .child(card_note(&theme, column_count))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .flex_1()
                                .min_w_0()
                                .child(self.panel.update(cx, |panel, cx| {
                                    panel.render_scenario_button("mock-detail-scenario-columns", cx)
                                })),
                        )
                        .child(self.panel.update(cx, |panel, cx| {
                            panel.render_column_filter(config_id.unwrap_or_default(), window, cx)
                        }))
                        .child(self.panel.update(cx, |panel, cx| {
                            panel.render_column_source_buttons(config_id.unwrap_or_default(), cx)
                        })),
                )
                .child(self.render_fields(window, cx));
        } else {
            // 结果 tab 也不给段标题：列数并进一行小字，和表配置 tab 的读法一致
            columns_block = columns_block
                .child(card_note(
                    &theme,
                    format!("{result_columns} 列 · 只读（来自这次生成）"),
                ))
                .child(self.render_result_columns(generated.as_ref(), cx));
        }

        // ── 预览 ──
        // 段 3：预览（标题行 + 表格 / 空态）。表格 `DataTable` 自带虚拟化与横向滚动，
        // 段内直接撑满——不再随整页滚。单表出口已上移到表头的动作行：这里只留预览。
        let mut preview_block = div().v_flex().gap_2().w_full().flex_1().min_h_0();

        // 有结果：标题行常驻的只有「现在看的是哪一段」（排序口径 + 前 N / 共 M 行）；
        // 临时表名 / 耗时 / 排序的重查语义是低频解释，一并收进 info 悬停
        // （表名与总行数已在表头事实行，标题行不重复）。
        if let Some(info) = generated.as_ref() {
            let title = match &preview_sort_label {
                // 排序生效时把口径说清：这是「按该列排序后的前 N 行」，不是生成顺序的前 N 行
                Some(label) => format!(
                    "预览（{label} · 前 {preview_shown} 行 · 共 {} 行）",
                    with_thousands(u64::from(info.row_count))
                ),
                None => format!(
                    "预览（前 {preview_shown} 行 · 共 {} 行）",
                    with_thousands(u64::from(info.row_count))
                ),
            };
            let temp_table = info.temp_table_name.clone();
            let elapsed_ms = info.elapsed_ms;
            // 排序生效时才解释重查语义（没排序时「前 N 行」不言自明）
            let sort_note = preview_sort_label.as_ref().map(|label| {
                format!(
                    "「{label}」是重查临时表得来的全局前 {preview_shown} 行，不是生成顺序的前几行；\
                     点表头右端的箭头可换向 / 取消，右键同一套。"
                )
            });
            let title_hover = HoverCard::new(ElementId::Name(SharedString::from(format!(
                "mock-preview-tech-{tab_key}"
            ))))
            .trigger(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(info_hint_icon(&theme)),
            )
            .content(move |_, _, _cx| {
                let mut card = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .text_xs()
                    .child(format!("临时表 {temp_table} · 生成耗时 {elapsed_ms} ms"));
                if let Some(note) = sort_note.clone() {
                    card = card.child(div().max_w(ui::STATUS_TOOLTIP_MAX_WIDTH).child(note));
                }
                card
            });
            preview_block = preview_block.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(title_hover)
                    .child(self.panel.update(cx, |panel, cx| {
                        panel.render_preview_limit_menu(&tab_key, cx)
                    })),
            );
        }

        // 表格只认「真有行可显示」：生成结果里有行才画 `DataTable`（预览表 = 组件库
        // `DataTable`，与编辑器结果集同一套原语：表头 / 列宽拖拽 / 横向滚动 / 虚拟化 /
        // 键盘选择都是组件的，本处只给容器与一个几十行的 delegate）。
        let has_preview_rows = generated.as_ref().is_some_and(|_| preview_shown > 0);
        if has_preview_rows {
            self.ensure_preview_table(window, cx);
            if let Some(table) = self.preview_table.clone() {
                preview_block = preview_block.child(
                    div()
                        .size_full()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .debug_selector(|| "mock-preview-table".to_string())
                        // 行高走组件档位（`ui::PREVIEW_TABLE_SIZE` = XSmall = 26px，与结果集网格同档），
                        // 单元格字号在 delegate 里取 `text_xs`（与面板其它行的密度一致 —— 这是组件库
                        // px 基准尺寸的既定例外）
                        .child(
                            DataTable::new(&table)
                                // 斑马纹 / 密度 / 内距与结果集网格对齐（同一类东西，观感不齐会在并列对照时显形）
                                .stripe(true)
                                .with_size(ui::PREVIEW_TABLE_SIZE),
                        ),
                );
            }
        } else {
            // 没有可显示的行：空态走 `Empty`（与右 Dock 模板 / 历史空态同一支组件——
            // 自带居中布局 + 虚线框 + 图标变体；此前这里是一行裸文本「（暂无预览）」，
            // 预览段是三段里最大的一块，一行灰字把视觉重心塌掉了）。
            // 「为什么空」各说各的引导（rds-design 空态文案规范：原因 + 下一步）；
            // 表配置 tab 顺手给「生成」主按钮——空态直达主操作，不必回表头找。
            let (title, desc) = if generated.is_some() {
                ("预览为空", "这张表没有可显示的行".to_string())
            } else if dropped_by_project_switch {
                (
                    "结果已随项目切换作废",
                    if is_draft {
                        "切项目会清空 mock 结果；表配置还在，重新生成后再看".to_string()
                    } else {
                        "切项目会清空 mock 结果——回到原项目重开，或重新生成".to_string()
                    },
                )
            } else if is_draft {
                // 空态本身带上「待生成什么」：列名就是这张表的结构，用户不用回列区确认
                // 自己配了什么。这也是「结果区默认显示（常驻）而不是生成后才出现」的
                // 依据：常驻但给信息，三段高度不随生成状态跳，分栏也不会因空态塌陷。
                let columns_note = config_id
                    .map(|id| {
                        let panel = self.panel.read(cx);
                        panel.config_for(id).columns.clone()
                    })
                    .map(|columns| {
                        let mut names: Vec<String> = columns
                            .iter()
                            .take(6)
                            .map(|column| column.def.name.clone())
                            .collect();
                        if columns.len() > names.len() {
                            names.push(format!("…共 {} 列", columns.len()));
                        }
                        names.join(" · ")
                    })
                    .filter(|note| !note.is_empty())
                    .unwrap_or_else(|| "还没有列（先加列再生成）".to_string());
                (
                    "还没有生成结果",
                    format!("{columns_note}——点「生成」在内存临时表里产出数据，预览跟着来"),
                )
            } else {
                (
                    "这一轮没有它的结果",
                    "从右 Dock 结果表清单重新生成，或切到表配置 tab 再跑".to_string(),
                )
            };
            // 生成按钮只给表配置 tab（结果表 tab 只读，不给「能只重跑这一张」的错觉）；
            // 任务进行中禁用（与表头「生成」同一口径，不靠任务层拒绝）
            let generate_action = if is_draft && generated.is_none() {
                let panel_entity = self.panel.clone();
                let running = self.panel.read(cx).is_running();
                Some(
                    button(
                        SharedString::from(format!("mock-preview-generate-{tab_key}")),
                        "生成",
                        ActionKind::Main,
                    )
                    .disabled(running)
                    .on_click(move |_, _, app| {
                        panel_entity.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.run_generate(cx)
                        });
                    }),
                )
            } else {
                None
            };
            preview_block = preview_block.child(
                Empty::new()
                    .header(
                        EmptyHeader::new()
                            // 收件箱 = 「数据还没到」的空态语义（与右 Dock 历史空态同一图标语言）
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(
                                        Icon::new(IconName::Inbox).size(rems(ui::PANEL_ICON_SIZE)),
                                    ),
                            )
                            .title(EmptyTitle::new().child(title))
                            .description(EmptyDescription::new().child(desc)),
                    )
                    .when_some(generate_action, |this, btn| {
                        // `debug_selector` 只是给用例找坐标（真点击检查；`.id(...)` 不登记坐标）
                        let sel_key = tab_key.clone();
                        this.content(
                            EmptyContent::new().child(
                                div()
                                    .debug_selector(move || {
                                        format!("mock-preview-generate-{sel_key}")
                                    })
                                    .child(btn),
                            ),
                        )
                    })
                    .min_h(rems(ui::PREVIEW_MIN_HEIGHT)),
            );
        }

        // 三段可拖分栏（`v_resizable`，仿编辑器结果区分栏先例）：基本信息 / 列区 / 预览。
        // 每段独立滚动（列区字段多、预览表格虚拟化，各滚各的）；拖拽高度经 `on_resize`
        // 回写面板字段（rem），跨 tab / 跨窗口沿用。初始大小：基本信息 100px（可拉到 220
        // 看全进度 / 失败重试 / 来源说明），预览按 `detail_preview_rem` 收住上限（默认
        // 20rem，不再吃满剩余），列区 flex 吃剩余——清单比预览更需要高度，窄窗口也不会溢出。
        let font_size = cx.theme().font_size;
        // 预览的高度上限（同时是拖拽范围末端）：面板字段由 `on_resize` 回写
        let preview_max = (self.panel.read(cx).detail_preview_rem * font_size)
            .max(ui::DETAIL_PREVIEW_MIN_HEIGHT)
            .min(ui::DETAIL_PREVIEW_MAX_HEIGHT);
        let split_panel = self.panel.clone();
        // 分栏 id 拼 tab 键：多个详情 tab 并存时各自独立（a11y 节点 + resizable state
        // 都按 id 区分；尺寸经 on_resize 回写面板字段，跨 tab 仍沿用）
        let split = v_resizable(format!("mock-detail-split-{tab_key}"))
            .child(
                resizable_panel()
                    .size(ui::DETAIL_HEADER_HEIGHT)
                    // 下界必须小于初始高度，否则往下拖一碰到 `min_h` 就停住（「拖不动」）
                    .size_range(ui::DETAIL_HEADER_MIN_HEIGHT..ui::DETAIL_HEADER_MAX_HEIGHT)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            // 段内内距：三段内容此前都贴着 tab 边缘与分隔条，
                            // 与分栏条的线挤在一起，看着像没排版的表配置
                            .p_2()
                            .overflow_y_scrollbar()
                            .child(head_block),
                    ),
            )
            .child(
                resizable_panel()
                    .size_range(ui::DETAIL_COLUMNS_MIN_HEIGHT..px(4000.))
                    // basis 0：不按内容高度撑开（面板默认 basis = auto，列一多会把预览
                    // 段推到视口外），高度完全由 flex 分配——清单比预览更需要高度
                    .flex_basis(px(0.))
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .p_2()
                            // 列区套一层卡片做边界：清单与预览之间没有框时，看起来像同一块
                            // 内容被分隔条切开；滚动留给框内，滚动条贴框内沿
                            .child(
                                card(&theme)
                                    .size_full()
                                    .min_h_0()
                                    .p_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .size_full()
                                            .min_h_0()
                                            .p_3()
                                            .overflow_y_scrollbar()
                                            .child(columns_block),
                                    ),
                            ),
                    ),
            )
            .child(
                resizable_panel()
                    .size(preview_max)
                    .size_range(ui::DETAIL_PREVIEW_MIN_HEIGHT..ui::DETAIL_PREVIEW_MAX_HEIGHT)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .p_2()
                            .flex()
                            .flex_col()
                            .child(preview_block),
                    ),
            )
            .on_resize(move |state, _window, app| {
                // 拖到哪记到哪：三段的第 3 段是预览（`sizes()[2]`），换算回 rem 存面板字段
                if let Some(h) = state.read(app).sizes().get(2).copied() {
                    let ratio = h / font_size;
                    split_panel.update(app, |panel, cx| {
                        panel.detail_preview_rem = ratio;
                        cx.notify();
                    });
                }
            });

        // 分栏占满 tab 的剩余高度：`ResizablePanelGroup` 自己是 `size_full()`，
        // 直接塞进 `body` 会按整块高度铺满并压过后面的兄弟；套一层 `flex_1().min_h_0()`
        // 才算「吃剩余」。不套时它按内容撑开，三段（含预览固定高度）合计超出窗口。
        body = body.child(div().flex_1().min_h_0().child(split));

        // 键位落点：`track_focus` 不能省（没有它这条 tab 不在 dispatch path 上，快捷键落不到动作）；
        // `key_context("mock-detail")` 与 `crates/app` 的键位注册（Ctrl+Enter = 生成）成对。
        body.track_focus(&self.focus_handle)
            .key_context("mock-detail")
            .on_action(cx.listener(Self::on_generate_key))
    }
}

/// 清单行里的**状态**：`IconName` 图标（走图标组件，不再用 Unicode 字形）+ 语义色文字。
///
/// 必须是 `h_flex`：根容器是块级 `div` 时图标、「 」、文字会各占一行垂直堆叠，
/// 一个状态格撑成三行高（与 `Button` 被塞进块级 div 会居中是同一类容器陷阱）。
/// 图标与文字的间距走 `gap_1`，不再靠插一个空格字符。
pub(crate) fn render_status_cell(icon: Option<IconName>, text: &str, color: Hsla) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .text_xs()
        .text_color(color)
        .when_some(icon, |cell, icon| {
            cell.child(
                Icon::new(icon)
                    .size(rems(ui::STATUS_ICON_SIZE))
                    .text_color(color),
            )
        })
        .child(SharedString::from(text))
}

/// 推给预览表的那一份快照：表名 + 列名 + 行 + 生效中的排序（`None` = 按生成顺序）。
///
/// 三个字段要一起换：只换行不换列会让右键记下的位置错位，只换排序不重建表头会让箭头与数据不一致。
pub(crate) struct PreviewSnapshot {
    /// 哪张表的结果（右键排序时按它去找临时表）
    pub(crate) table: String,
    /// 数据列名（不含行号槽）
    pub(crate) columns: Vec<String>,
    /// 取样行（引擎已渲染成文本；每行长度 == `columns.len()`）
    pub(crate) rows: Vec<Vec<String>>,
    /// 生效中的排序：列名 + 是否降序（面板那边算好的，delegate 只负责映射成列下标）
    pub(crate) sort: Option<(String, bool)>,
}

/// 预览表的 delegate：把「这次生成的取样」铺进组件库的表格（`DataTable`）。
///
/// **为何不手搓网格**：结果集（编辑器）与这里的预览是同一类东西（一张只读的宽表），
/// 组件库的 `DataTable` 已提供表头 / 列宽拖拽 / 横向滚动 / 虚拟化 / 键盘选择。
/// 旧实现自己搭了一个固定 9rem 列宽的 div 表：列宽钉死、值截断后没任何办法看全，
/// 而且与结果集两套观感（尺寸与交互各要同步一次）。
pub(crate) struct PreviewTableDelegate {
    /// 面板实体（右键排序与表头排序都要把「这一列 + 方向」交给它去重查）
    pub(crate) panel: WeakEntity<MockPanel>,
    /// 本 tab 看的表名
    pub(crate) table: String,
    /// 数据列名（不含行号槽）
    pub(crate) columns: Vec<String>,
    /// 取样行（引擎已渲染成文本；每行长度 == `columns.len()`）
    pub(crate) rows: Vec<Vec<String>>,
    /// 生效中的排序（列下标 + 方向；`None` = 没排序）：表头的箭头就画在它上
    pub(crate) sort: Option<(usize, ColumnSort)>,
    /// 用户拖过的列宽（**按列名**记，不是下标）：重建表头时按名字恢复。
    ///
    /// 为什么必须自己记：列宽归组件（`col_groups` 是 `TableState` 的私有状态），
    /// 而每次重建表头（重查取样 / 重新生成）都会从 `column()` 重新取宽——不记就等于
    /// 把用户拖好的宽度打回默认档。事件 [`TableEvent::ColumnWidthsChanged`] 拖完就发，
    /// 订阅在详情视图那边（`ensure_preview_table`）。
    pub(crate) widths: HashMap<String, Pixels>,
    /// 右键落在哪个单元格（组件库只把 `row_ix` 交给 `context_menu`，
    /// 列得在 `render_td` 里自己记一笔——与结果集网格同一做法）
    pub(crate) context_cell: Option<(usize, usize)>,
}

impl PreviewTableDelegate {
    /// 首列是行号槽（与结果集同形：`#` 钉在左侧，不可拖宽 / 移动 / 选中）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 建 delegate（列 / 行 / 排序一次到位：`TableState::new` 当场就会问 `column`）。
    pub(crate) fn new(panel: WeakEntity<MockPanel>, snapshot: PreviewSnapshot) -> Self {
        let PreviewSnapshot {
            table,
            columns,
            rows,
            sort,
        } = snapshot;
        let sort = Self::map_sort(&columns, sort);
        Self {
            panel,
            table,
            columns,
            rows,
            sort,
            widths: HashMap::new(),
            context_cell: None,
        }
    }

    /// 记下组件报来的全部列宽（下标 → 列名；行号槽的宽是钉死的，不记）。
    ///
    /// 列集合变了就把不再存在的名字剔掉（同一次会话里换过表也不会无限长）。
    fn set_widths(&mut self, all: &[Pixels]) {
        let mut widths: HashMap<String, Pixels> = HashMap::new();
        for (ix, width) in all.iter().enumerate() {
            let Some(index) = ix.checked_sub(1) else {
                continue;
            };
            let Some(name) = self.columns.get(index) else {
                continue;
            };
            widths.insert(name.clone(), *width);
        }
        self.widths = widths;
    }

    /// 面板给的「生效中的排序」（列名 + 方向）→ 列下标 + 方向。
    ///
    /// 列名对不上（列被换了）就当没有排序：箭头不画，也不去重查一个不存在的列。
    fn map_sort(columns: &[String], sort: Option<(String, bool)>) -> Option<(usize, ColumnSort)> {
        let (column, descending) = sort?;
        let index = columns.iter().position(|name| name == &column)?;
        Some((
            index,
            if descending {
                ColumnSort::Descending
            } else {
                ColumnSort::Ascending
            },
        ))
    }

    /// 换一份快照；返回是否真的变了（没变就不必 `refresh`：生成中面板每 120ms 通知一次）。
    ///
    /// 排序也是快照的一部分：面板拒绝重查时会把**上一份**排序回推过来，这里就会看到它与
    /// 表头刚才翻到的方向不一致（`perform_sort` 先照抄了一份）——于是 `refresh` 把箭头纠回来。
    pub(crate) fn set_preview(&mut self, snapshot: PreviewSnapshot) -> bool {
        let PreviewSnapshot {
            table,
            columns,
            rows,
            sort,
        } = snapshot;
        let sort = Self::map_sort(&columns, sort);
        if self.table == table && self.columns == columns && self.rows == rows && self.sort == sort
        {
            return false;
        }
        // 列集合换了：与它们对不上的记录宽度没有意义（留着只会在同名列回来时意外生效）
        if self.columns != columns {
            self.widths.retain(|name, _| columns.contains(name));
        }
        self.table = table;
        self.columns = columns;
        self.rows = rows;
        self.sort = sort;
        // 数据换了，之前记下的右键位置不再指向同一个值
        self.context_cell = None;
        true
    }

    /// 这一行拼成 TSV（「复制整行」用：拷进表格 / SQL 都能直接分列）。
    ///
    /// 逐格转义走 `shared::string::tsv_row`（**唯一实现**，结果集网格的右键复制也调它）：
    /// 取值里的制表符不转义就会把一行错成两列——“粘进表格就能直接分列”那时就不成立了。
    pub(crate) fn row_text(&self, row_ix: usize) -> String {
        self.rows
            .get(row_ix)
            .map(|row| tsv_row(row))
            .unwrap_or_default()
    }

    /// 右键那一格的数据列下标：优先用记下的单元格（行号槽除外），否则退到第一列。
    pub(crate) fn context_col(&self, row_ix: usize) -> Option<usize> {
        match self.context_cell {
            Some((cell_row, cell_col)) if cell_row == row_ix && cell_col > 0 => Some(cell_col - 1),
            _ if !self.columns.is_empty() => Some(0),
            _ => None,
        }
    }

    /// 右键菜单里拿得到的那一格。
    pub(crate) fn context_text(&self, row_ix: usize) -> Option<String> {
        let col = self.context_col(row_ix)?;
        self.rows.get(row_ix)?.get(col).cloned()
    }

    /// 右键那一格的列名（排序菜单项要它）。
    pub(crate) fn context_column(&self, row_ix: usize) -> Option<String> {
        let col = self.context_col(row_ix)?;
        self.columns.get(col).cloned()
    }

    /// 这一数据列在**取样窗口内**的全部取值（每行一个，拿去用换行拼成单列 TSV）。
    ///
    /// 只给取样里的值、不重查整列：与「预览是取样」同一口径；而一张十万行的表整列
    /// 可能是十万个值，粘到哪里都不好用（真要全列，落库 / 导出后去查那张表）。
    ///
    /// 返回的每个取值**已经按 TSV 规则转义**（`shared::string::tsv_cell`，与结果集网格
    /// 「复制此列」同一份实现）：取值自身带换行时不转义就会被拼成两行。
    pub(crate) fn column_text(&self, index: usize) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|cell| tsv_cell(cell))
            .collect()
    }
}

impl TableDelegate for PreviewTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        // 没列时不画行号槽（空表就交给「（暂无预览）」那条空态）
        if self.columns.is_empty() {
            0
        } else {
            self.columns.len() + 1
        }
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if Self::is_row_number(col_ix) {
            return Column::new("row-no", "#")
                .width(ui::PREVIEW_ROW_NUMBER_WIDTH)
                .min_width(ui::PREVIEW_ROW_NUMBER_WIDTH)
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false);
        }
        let index = col_ix - 1;
        let name = self.columns.get(index).cloned().unwrap_or_default();
        // 列宽：拖过的按列名恢复，没拖过的走组件默认档（100px）+ 可拖宽——
        // 宽表靠横向滚动，看不全就拖
        let mut column = Column::new(format!("col-{index}"), name.clone());
        if let Some(width) = self.widths.get(&name) {
            column = column.width(*width);
        }
        // 可排序：表头挂排序箭头，点一下就把「这一列 + 方向」交给面板去重查。
        // 生效中的那一列把方向画出来（↑/↓），其余画「可点」的默认箭头。
        match self.sort {
            Some((sorted_ix, sort)) if sorted_ix == index => column.sort(sort),
            _ => column.sortable(),
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let fg = cx.theme().colors.foreground;
        let text = self.cell_text(row_ix, col_ix, cx);
        // 行号槽永远是灰的（它不是数据）
        let is_row_number = Self::is_row_number(col_ix);
        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（与结果集网格同口径）
        let is_null = !is_row_number && text == "NULL";
        let color = if is_row_number || is_null { muted } else { fg };
        // 右键得先知道「点的是哪一格」：组件库只把 `row_ix` 交给 `context_menu`
        // （事件从最内层派发，不会丢）——与结果集网格同一做法
        let table = cx.entity().clone();
        let mut cell = div()
            .id((
                "mock-preview-cell",
                row_ix * (self.columns.len() + 1) + col_ix,
            ))
            .px_2()
            .truncate()
            .text_xs()
            .text_color(color)
            .when(is_null, |cell| cell.italic())
            // 行号槽的单元格挂 `h_full` + 选择器：用例按它量**行距**（表格密度那条口径的哨兵，
            // 与结果集网格同一条用例；`.id()` 不登记坐标）
            .when(is_row_number, |cell| {
                cell.h_full()
                    .debug_selector(move || format!("mock-preview-rowno-{row_ix}"))
            })
            .when(!is_row_number, |cell| {
                cell.on_mouse_down(MouseButton::Right, move |_, _window, app| {
                    table.update(app, |state, _cx| {
                        state.delegate_mut().context_cell = Some((row_ix, col_ix));
                    });
                })
            });
        // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路。
        //
        // 为什么要 `id`：gpui 的**流式** `tooltip` 声明在 `StatefulInteractiveElement` 上
        // （没 id 的裸 `div` 只有直调 `Interactivity` 的写法）；组件库自己的表头/行头单元格
        // 也是这么挂 id 的。行号槽不给悬停提示：那是序号，不是数据。
        if !is_row_number && !text.is_empty() {
            let full = text.clone();
            cell = cell.tooltip(move |window, cx| preview_cell_tooltip(full.clone(), window, cx));
        }
        cell.child(text)
    }

    /// 表头单元格：组件默认只画列名（`div().size_full().child(name)`），这里补两件事——
    /// **截断**与**悬停全文**。
    ///
    /// 为什么值得自己画：列名常常比默认列宽（100px）长，截断之后一样没有任何办法知道它是什么；
    /// 单元格取值已经有悬停全文（`preview_cell_tooltip`），表头同样需要。
    /// 行号槽（`#`）不挂悬停——它不是数据。
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = if Self::is_row_number(col_ix) {
            "#".to_string()
        } else {
            self.columns.get(col_ix - 1).cloned().unwrap_or_default()
        };
        // `debug_selector` 是给用例找这个表头用的（`.id(...)` 不登记坐标）：
        // 「点列头还能排序」靠真点击验证，不靠直接调 `perform_sort`。
        let head = div()
            .size_full()
            .truncate()
            .debug_selector(move || format!("mock-preview-th-{col_ix}"))
            .child(name.clone());
        if Self::is_row_number(col_ix) || name.is_empty() {
            return head.into_any_element();
        }
        head.id(("mock-preview-th", col_ix))
            .tooltip(move |window, cx| preview_cell_tooltip(name.clone(), window, cx))
            .into_any_element()
    }

    /// 表头的**排序箭头**点击 → 按这一列**重查临时表**（不是把手上这几行换个顺序）。
    ///
    /// 入口是表头右端那个箭头，不是表头正文：组件 0.7.1 的 `on_col_head_click` 只做列选择
    /// （我们关掉了），排序只挂在 `render_sort_icon` 上——与结果集网格同一处入口。
    ///
    /// 组件库已经把方向循环算好了（默认 → 降序 → 升序 → 默认）并先改了自己的箭头，
    /// 这里做两件事：
    /// ① 把这个方向照抄进 delegate——面板那一拍若拒绝（内存库忙 / 列不存在），
    ///    它会把**上一份**排序回推过来，`set_preview` 就会看到差别并 `refresh` 把箭头纠回实际状态；
    /// ② 把「这一列 + 方向」交给面板：重查是 I/O，而临时表名与宿主端口都在它手里。
    ///
    /// 「默认」那一档（点第三下）= 取消排序，回到生成时的前 N 行。
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // 行号槽（`Column` 没开 sortable）不会走到这里；真到了也只当没这回事
        let Some(index) = col_ix.checked_sub(1) else {
            return;
        };
        let Some(column) = self.columns.get(index).cloned() else {
            return;
        };
        self.sort = Some((index, sort));
        let panel = self.panel.upgrade();
        let Some(panel) = panel else {
            return;
        };
        let table = self.table.clone();
        let descending = match sort {
            ColumnSort::Ascending => Some(false),
            ColumnSort::Descending => Some(true),
            ColumnSort::Default => None,
        };
        panel.update(cx, |panel, cx| {
            panel.sort_preview(&table, &column, descending, cx)
        });
    }

    /// 预览的右键菜单：这一格能做的事——按这一列重排取样 / 复制（这一格 · 这一列 · 整行）。
    ///
    /// 排序进菜单的理由：右键落点就是「这一列」，比表头那个小箭头好找。
    /// 它走的是与表头点击同一条路：**重查**临时表取「按该列排序后的前 N 行」
    /// （见 [`MockPanel::sort_preview`]），不是就地重排手上这几行。
    ///
    /// 仍然不做筛选（结果集网格有）：筛选把取样变成长度未知的子集，
    /// 而「前 N 行长什么样」正是这份取样唯一的信息。
    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let mut menu = menu;
        if let Some(column) = self.context_column(row_ix) {
            let panel = self.panel.clone();
            let table = self.table.clone();
            menu = menu
                .item(preview_sort_menu_item(
                    format!("按「{column}」升序"),
                    panel.clone(),
                    table.clone(),
                    column.clone(),
                    Some(false),
                ))
                .item(preview_sort_menu_item(
                    format!("按「{column}」降序"),
                    panel.clone(),
                    table.clone(),
                    column.clone(),
                    Some(true),
                ));
            // 取消排序只在真有排序时出现（没排序时它只是个噪声项）
            if self.sort.is_some() {
                menu = menu.item(preview_sort_menu_item(
                    "取消排序".to_string(),
                    panel,
                    table,
                    column,
                    None,
                ));
            }
        }
        if let Some(value) = self.context_text(row_ix) {
            menu = menu.item(
                PopupMenuItem::new("复制此值").on_click(move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(value.clone()));
                }),
            );
        }
        // 整列：取样窗口内的全部取值（一行一个，逐格已按 TSV 转义）——粘进表格就是一列
        if let Some(index) = self.context_col(row_ix) {
            let values = self.column_text(index);
            if !values.is_empty() {
                let label = format!("复制此列（{} 个取值）", values.len());
                let text = values.join("\n");
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
                }));
            }
        }
        let row_text = self.row_text(row_ix);
        if !row_text.is_empty() {
            menu = menu.item(PopupMenuItem::new("复制整行（TSV）").on_click(
                move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(row_text.clone()));
                },
            ));
        }
        menu
    }

    /// 取样的文本表示：组件库用它做 `dump` / 导出（预览的取值本来就是文本，直接给）。
    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    }
}

/// 单元格的悬停全文：取值可能很宽（JSON / 长文本），`truncate` 之后只剩这一条看全的路。
///
/// 宽度走 `ui::PREVIEW_TOOLTIP_MAX_WIDTH`（结构尺寸只认常量表），并让它**折行**：
/// 悬停提示跟着鼠标，横向越宽越容易被窗口边缘截掉。
fn preview_cell_tooltip(text: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        div()
            .max_w(rems(ui::PREVIEW_TOOLTIP_MAX_WIDTH))
            .whitespace_normal()
            .text_xs()
            .child(text.clone())
    })
    .build(window, cx)
}

/// 右键菜单里的排序项：与表头点击同一条路（把「这一列 + 方向」交给面板去重查临时表）。
///
/// `descending = None` = 取消排序（回到生成时的前 N 行）。
fn preview_sort_menu_item(
    label: String,
    panel: WeakEntity<MockPanel>,
    table: String,
    column: String,
    descending: Option<bool>,
) -> PopupMenuItem {
    PopupMenuItem::new(label).on_click(move |_, _window, app| {
        let Some(panel) = panel.upgrade() else {
            return;
        };
        panel.update(app, |panel, cx| {
            panel.sort_preview(&table, &column, descending, cx)
        });
    })
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
