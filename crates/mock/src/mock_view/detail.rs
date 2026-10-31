// ==================== 详情视图（中央 tab：字段 + 预览） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 样例预览的固定种子：样例稳定可复现（与最终生成的随机种子无关，纯示意）
use fake::rand::rngs::StdRng;
use fake::rand::SeedableRng;
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use std::time::Duration;

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

/// 列编辑对话框的工作副本（「应用」才写回草稿）。
pub(crate) struct ColumnDraft {
    /// 既有列的 id
    id: u64,
    /// 列名输入
    name_input: Entity<InputState>,
    pub(crate) def: ColumnDef,
    pub(crate) confidence: String,
    sample_value: String,
    /// 参数实时样例预览（按当前输入框值生成 2 条；固定种子，可复现）
    pub(crate) sample_preview: String,
    pub(crate) params: Vec<ParamInput>,
    null_input: Entity<InputState>,
    type_label: String,
    /// 复杂参数解析失败的原因（键 + 文案）：只在对话框里就地提示，不写回生成器
    pub(crate) param_error: Option<(String, String)>,
}

/// 中央详情 tab 的**身份**：一张结果表一个 tab，另有草稿 tab（列定义可编辑那张）。
///
/// 为何用名字而不是下标做身份：tab 是长命的东西，而 `results` 每次生成都会重建
/// （下标会指向别的表）；名字才是用户看到的那张表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailTarget {
    /// 草稿 tab：单表态的列定义（可编辑）+ 草稿目标表的预览
    Draft,
    /// 结果表 tab：该表的列（只读）与预览
    Table(String),
}

impl DetailTarget {
    /// 宿主登记 tab 用的键（同一个键只开一个 tab）。
    pub fn key(&self) -> String {
        match self {
            Self::Draft => "draft".to_string(),
            Self::Table(name) => format!("table:{name}"),
        }
    }

    /// 日志 / 测试用的短名。
    pub fn label(&self) -> String {
        match self {
            Self::Draft => "草稿".to_string(),
            Self::Table(name) => name.clone(),
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
    let progress = progress?;
    Some(if progress.phase.is_quantified() {
        format!("◐ Mock 生成中 {:.0}%", progress.percent())
    } else {
        format!("◐ Mock {}", progress.phase.label())
    })
}

/// Mock 详情（中央编辑区 tab）：字段清单（编辑走对话框）+ 预览表格。
///
/// 状态单一权威：持有配置面板实体，字段与预览都从它读取；编辑动作写回面板。
/// **一个 tab 对应一个目标**（草稿或某张结果表，见 [`DetailTarget`]）：tab 被激活就等于
/// 把那张表设为「当前表」，出口（落库 / 追加 / 导出）跟着它走。
pub struct MockDetailView {
    panel: Entity<MockPanel>,
    /// 这个 tab 管的是哪张表 / 草稿
    target: DetailTarget,
    /// 列编辑的工作副本（右栏内嵌，主界面不弹窗）
    pub(crate) draft: Option<ColumnDraft>,
    /// 正在编辑的列 id（`None` = 未在编辑列；`draft` 与之成对出现）
    pub(crate) editing_column_id: Option<u64>,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`Panel::on_added_to` 注入；重复点「查看详情」时用于聚焦自身）
    pub(crate) group: Option<WeakEntity<TabGroup>>,
    /// 预览表状态（组件库 `DataTable` 的 `TableState`）：懒创建（`TableState::new` 要 window），
    /// 数据由面板通知那一拍同步（见 [`MockDetailView::sync_preview_table`]）。
    pub(crate) preview_table: Option<Entity<TableState<PreviewTableDelegate>>>,
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
            editing_column_id: None,
            focus_handle: cx.focus_handle(),
            group: None,
            preview_table: None,
        }
    }

    /// 本 tab 看的那张表（草稿 tab = 草稿的目标表；表名为空时没有可看的东西）。
    fn target_table(&self, panel: &MockPanel) -> Option<String> {
        match &self.target {
            DetailTarget::Draft => {
                let name = panel.draft().table_name.clone();
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
    /// 只在**草稿 tab** 上生效：结果表 tab 没有「生成」这个动作（D38——它是这次生成的产物，
    /// 要改就回草稿改完再生成）。进行中静默忽略，与按钮的禁用态同语义
    /// （不为一次按键弹一句「已有任务在进行中」）。
    pub(crate) fn on_generate_key(
        &mut self,
        _: &crate::commands::GenerateMock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.target, DetailTarget::Draft) {
            return;
        }
        self.panel.update(cx, |panel, cx| {
            if !panel.is_running() {
                panel.run_generate(cx);
            }
        });
    }

    /// 这个 tab 管的目标（宿主建 tab / 测试用）。
    pub fn target(&self) -> &DetailTarget {
        &self.target
    }

    /// tab 标题：结果表写 `表名（N 行）`，草稿写目标表名（与旧行为一致）。
    ///
    /// 行数实时取自面板结果，所以重新生成 / 「编辑表」改完行数后标题会自己更新。
    pub fn tab_label(&self, cx: &App) -> String {
        let panel = self.panel.read(cx);
        match &self.target {
            DetailTarget::Draft => format!("Mock · {}", panel.draft().table_name),
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
        })
    }

    /// 打开「列编辑」（主界面内嵌右栏，不弹窗）：列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认。
    pub fn open_column_editor(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(column) = self
            .panel
            .read(cx)
            .draft()
            .columns
            .iter()
            .find(|c| c.id == id)
            .cloned()
        else {
            return;
        };

        let null_initial = format!("{}", (column.def.nullable_ratio * 100.0).round() as i64);
        let null_input = cx.new(|cx| InputState::new(window, cx).placeholder("0~100"));
        null_input.update(cx, |s, cx| s.set_value(null_initial, window, cx));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("列名"));
        name_input.update(cx, |s, cx| s.set_value(column.def.name.clone(), window, cx));

        self.draft = Some(ColumnDraft {
            id,
            name_input,
            type_label: column_type_label(&column.def.data_type),
            def: column.def.clone(),
            confidence: column.confidence.clone(),
            sample_value: column.sample_value.clone(),
            params: Vec::new(),
            null_input,
            param_error: None,
            sample_preview: String::new(),
        });
        // 参数输入行按当前生成器建立（生成器身份变化后由 `rebuild_params` 重建）
        self.rebuild_params(window, cx);
        // 初始样例预览（按当前配置生成，事件路径，不在 render 里做）
        self.refresh_sample_preview(cx);
        self.editing_column_id = Some(id);
        // 左栏常驻生成器库的写回目标指向当前编辑列（随编辑目标切换；实体在
        // `MockPanel::ensure_library` 创建）。选中生成器 → 写回草稿（`ColumnDraft`），
        // 点「应用」才落地——与对话框版同口径。
        if let Some(library) = self.panel.read(cx).library() {
            let detail = cx.entity().downgrade();
            library.update(cx, |s, cx| {
                s.writeback = crate::mock_view::generator_library::LibraryWriteback::ColumnDraft {
                    detail: Some(detail),
                    column_id: id,
                };
                cx.notify();
            });
        }
        cx.notify();
    }

    /// 关闭列编辑（取消 / 应用落地后）：丢弃工作副本，右栏回到三段分栏。
    fn close_column_editor(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.editing_column_id = None;
        cx.notify();
    }

    /// 按当前生成器重建参数输入行（保留同名参数的输入框与订阅）。
    /// 列编辑对话框内选了生成器（库对话框写回草稿，`LibraryWriteback::ColumnDraft`）：
    /// 写 draft 的生成器配置并重建参数区 / 样例预览。列还没应用，面板列不动——
    /// 与应用按钮同口径（改动留在草稿，点「应用」才落地）。
    pub(crate) fn apply_generator_config_to_draft(
        &mut self,
        column_id: u64,
        config: GeneratorConfig,
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
        draft.confidence = "manual".to_string();
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
        let fields: Vec<ParamField> = generator_catalog::spec_of(&generator)
            .params
            .iter()
            .copied()
            .collect();
        let mut previous: HashMap<String, ParamInput> =
            draft.params.drain(..).map(|p| (p.key.clone(), p)).collect();
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
    /// 固定种子保证样例稳定可读（与最终生成的随机种子无关，纯示意）。
    fn refresh_sample_preview(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let locale = self.panel.read(cx).draft().options.locale.clone();
        let mut rng = StdRng::seed_from_u64(SAMPLE_PREVIEW_SEED);
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
    pub(crate) fn visible_columns(&self, panel: &MockPanel, cx: &App) -> Vec<MockColumnSpec> {
        let query = panel.column_filter_query(cx);
        panel
            .draft()
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

    fn render_fields(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let info = cx.theme().colors.info;

        // 先看「这张表有没有列」再套筛选：空表与「筛没了」是两句不同的话
        let has_any = {
            let panel = self.panel.read(cx);
            !panel.draft().columns.is_empty()
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
            self.visible_columns(panel, cx)
        };
        if columns.is_empty() {
            return div()
                .text_xs()
                .text_color(muted)
                .child("没有匹配的列——清空「筛选列」看全部（匹配列名与生成器名）。")
                .into_any_element();
        }

        let mut list = div().v_flex().gap_2().w_full();
        // enumerate 拿序号：首/末列禁用对应的 ↑/↓（列序 = 生成顺序）
        for (index, column) in columns.iter().enumerate() {
            let spec = generator_catalog::spec_of(&column.def.generator);
            let summary = summarize_params(&column.def.generator);
            let badge = match column.confidence.as_str() {
                "high" => "high",
                "manual" => "manual",
                _ => "low",
            };
            let id = column.id;
            let label = spec.label;
            let unique = if column.def.unique { "唯一 · " } else { "" };
            let null_percent = (column.def.nullable_ratio * 100.0).round() as i64;
            let detail = if summary.is_empty() {
                column.sample_value.clone()
            } else {
                summary.clone()
            };
            // S1④：跨表引用徽标 + 常驻说明（挂在列上的关系，不是外键 DDL）
            let ref_badge = column.def.dependency.as_ref().map(|dep| {
                format!("↳ 引用 {}.{}", dep.ref_table, dep.ref_column)
            });
            let ref_note = column.def.dependency.is_some().then(|| {
                "引用目标仅支持自增主键，按自增规律推算取值域，不读取父表已生成的数据。"
            });
            let panel = self.panel.clone();
            list = list.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-field-{id}"
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
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
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(column.def.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(column_type_label(&column.def.data_type)),
                            )
                            .child({
                                // 置信度徽标 = 行内 chip（`tag::Tag` outline 形态，克制不抢列名）：
                                // high=成功绿 / manual=提示蓝 / low=次级灰。
                                // 不用 `badge::Badge`——它是角标（absolute 浮点），不是行内文字标签
                                let tag = match badge {
                                    "high" => Tag::success(),
                                    "manual" => Tag::info(),
                                    _ => Tag::secondary(),
                                };
                                tag.outline().small().child(badge)
                            })
                            .child({
                                // 引用徽标：挂关系的列一眼可见「引用谁」
                                let mut badge_el = div().flex_none().text_xs();
                                if let Some(text) = &ref_badge {
                                    badge_el = badge_el.text_color(info).child(text.clone());
                                }
                                badge_el
                            }),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-gen-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label(label)
                                .tooltip("打开生成器库（业务分类 / 数据类型双视角选择）")
                                .tooltip_show_delay(Duration::from_millis(500))
                                .on_click(move |_, window, app| {
                                    panel.update(app, |p, cx| {
                                        p.open_generator_library(id, window, cx);
                                    });
                                })
                            })
                            .child({
                                let entity = cx.entity();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-edit-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("编辑")
                                .on_click(
                                    move |_, window, app| {
                                        entity.update(app, |view, cx| {
                                            view.open_column_editor(id, window, cx);
                                        });
                                    },
                                )
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-smart-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("智能")
                                .tooltip("按列名与类型重跑智能映射（恢复推荐生成器与参数）")
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| {
                                        panel.reset_column_mapping(id, cx)
                                    });
                                })
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-del-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("删除")
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| panel.remove_column(id, cx));
                                })
                            })
                            .child({
                                // 列排序：icon-only（↑/↓），首/末列禁用对应方向；列序即生成顺序
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-moveup-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ArrowUp)
                                .tooltip("上移（调整生成顺序）")
                                .disabled(index == 0)
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| panel.move_column(id, true, cx));
                                })
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-movedown-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ArrowDown)
                                .tooltip("下移（调整生成顺序）")
                                .disabled(index + 1 == columns.len())
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| panel.move_column(id, false, cx));
                                })
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-copy-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .tooltip("复制列（快速造相似字段，复制后再微调参数）")
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| panel.duplicate_column(id, cx));
                                })
                            })
                            .child({
                                // 参数摘要一行 ellipsis 截断；悬停 `HoverCard` 看完整参数与生成器。
                                // 触发容器样式走 `trigger_style`（组件负责布局该容器）
                                let summary_text = format!("{unique}空值 {null_percent}% · {detail}");
                                let full_text = if summary.is_empty() {
                                    format!("生成器：{}", spec.label)
                                } else {
                                    format!("生成器：{}\n{}", spec.label, summary)
                                };
                                HoverCard::new(ElementId::Name(SharedString::from(format!(
                                    "mock-hover-{id}"
                                ))))
                                .open_delay(std::time::Duration::from_millis(300))
                                .trigger_style(StyleRefinement::default().flex_1().min_w_0())
                                .trigger(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .text_ellipsis()
                                        .child(summary_text),
                                )
                                .content(move |_, _, cx| {
                                    // content 是 `Fn`（可多次调用）：每次克隆文本，不消耗捕获值
                                    div()
                                        .p_2()
                                        .text_xs()
                                        .text_color(cx.theme().colors.muted_foreground)
                                        .child(full_text.clone())
                                })
                            }),
                    )
                    .child({
                        // S1④：引用列常驻说明（解释「为什么不读父表数据」）
                        let mut note = div().w_full().text_xs();
                        if let Some(text) = &ref_note {
                            note = note.text_color(muted).child(*text);
                        }
                        note
                    }),
            );
        }

        div()
            .id("mock-field-list")
            .v_flex()
            .gap_2()
            .w_full()
            .max_h(rems(ui::FIELD_LIST_MAX_HEIGHT))
            .overflow_y_scrollbar()
            .child(list)
            .into_any_element()
    }
    /// 结果表 tab 的列清单（**只读**）：列来自这次生成（`MockGenInfo.columns`）。
    ///
    /// 为何不给编辑入口：这张表的列是「这次生成」的产物（模板或草稿的结果），
    /// 就地改列会让「结果」与「产出它的配置」分叉——要改列回草稿 tab 改，再重新生成。
    fn render_result_columns(
        &self,
        info: Option<&MockGenInfo>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let radius = cx.theme().radius;

        let mut block = div().v_flex().gap_1().w_full();
        let Some(info) = info else {
            return block.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("这一轮没有这张表的列信息（重新生成后再看）。"),
            );
        };
        block = block.child(div().text_xs().text_color(muted).child(format!(
            "列（{}）· 只读：它们是这次生成的产物；要改列请回草稿 tab 改完重新生成",
            info.columns.len()
        )));
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
            block = block.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(border)
                    .rounded(radius)
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(def.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(column_type_label(&def.data_type)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(summarize_params(&def.generator)),
                    )
                    .children(
                        reference
                            .map(|text| div().flex_none().text_xs().text_color(muted).child(text)),
                    )
                    .children((!meta.is_empty()).then(|| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(meta.join(" · "))
                    })),
            );
        }
        block
    }
}

impl Render for MockDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let warning = cx.theme().colors.warning;
        let border = cx.theme().colors.border;
        let is_draft = matches!(self.target, DetailTarget::Draft);
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
            // 本 tab 看哪张表：草稿 tab 看草稿目标表，结果表 tab 看它自己
            // （与面板的「当前表」无关——切 tab 就能对照两张表的预览）
            let table = match &self.target {
                DetailTarget::Draft => panel.draft().table_name.clone(),
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
                panel.draft().columns.len(),
                // 字段区的筛选计数与渲染读同一处（`visible_columns`），不另写一份口径
                self.visible_columns(panel, cx).len(),
                result_columns,
                panel.results_dropped_by_project_switch(),
                shown.and_then(|(_, sort)| sort).map(|sort| sort.label()),
                shown.map_or(0, |(preview, _)| preview.rows.len()),
            )
        };

        // ── 表头 ──
        // 草稿 tab：这张表的设计与生成状态（表名 / 行数 / 种子 / 语言 + 生成 + 进度 + 失败重试）。
        // 状态归面板（单一权威），画在中央——它属于这张表，不属于「有哪些表」那份清单。
        // 结果表 tab：只读事实 + 跨表后果；不给「生成」，免得像能只重跑这一张。
        let head = if is_draft {
            self.panel
                .update(cx, |panel, cx| panel.render_design_head(window, cx))
        } else {
            let facts = match generated.as_ref() {
                Some(info) => format!(
                    "{} · {} 行 · 已生成（临时表 {} · {} ms）",
                    table,
                    with_thousands(u64::from(info.row_count)),
                    info.temp_table_name,
                    info.elapsed_ms
                ),
                // 没结果分两种：切项目把结果清掉了（旧 tab 留着，见 D35），或者这一轮没生成它。
                // 两种说法的行动建议一样（重新生成），但「为什么没结果」不一样——不能都说成「这一轮」。
                None if dropped_by_project_switch => format!(
                    "{table} · 所属项目已切换：这张表的结果已作废（切项目会清空 mock 结果）——重新生成后再看"
                ),
                None => format!("{table} · 这一轮没有它的结果（重新生成后再看）"),
            };
            let mut head = div().v_flex().gap_1().w_full().child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(facts),
            );
            if let Some(source) = scenario_source.clone() {
                head = head.child(div().text_xs().text_color(muted).child(format!(
                    "来自场景模板「{source}」：列与预览都属于它自己；看别的表切 tab（或在右 Dock 结果表清单点一行）"
                )));
            }
            if let Some(note) = relation_note.clone() {
                head = head.child(div().text_xs().text_color(warning).child(note));
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
        // 三种态各自成块：空态卡片 / draft 标题行 + 字段列表 / 结果表只读列
        let columns_block = if is_draft && draft_columns == 0 {
            // 空态：三种起手方式都摆在这张 tab 上（右 Dock 只管表与集合出口）
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("还没有列：先把这张表造起来"),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(self.panel.update(cx, |panel, cx| {
                            panel.render_scenario_button("mock-detail-scenario-empty", cx)
                        }))
                        .child(self.panel.update(cx, |panel, cx| {
                            panel.render_column_source_buttons(cx)
                        })),
                )
                .child(div().text_xs().text_color(muted).child(
                    "列定义、表名 / 行数、预览都在这张 tab 里改；右侧面板负责管理表（清单 / 批量出口 / 历史）。",
                ))
        } else if is_draft {
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(div().flex_1().min_w_0().text_xs().text_color(muted).child(
                            if draft_columns_shown == draft_columns {
                                format!("列（{draft_columns}）")
                            } else {
                                format!("列（{draft_columns_shown} / {draft_columns}）· 已筛选")
                            },
                        ))
                        .child(
                            self.panel
                                .update(cx, |panel, cx| panel.render_column_filter(window, cx)),
                        )
                        .child(
                            self.panel
                                .update(cx, |panel, cx| panel.render_column_source_buttons(cx)),
                        ),
                )
                .child(self.render_fields(cx))
        } else {
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("列（{result_columns}）· 来自这次生成（只读）")),
                )
                .child(self.render_result_columns(generated.as_ref(), cx))
        };

        // ── 预览 ──
        // 标题行：口径说明（排序 / 行数）+ 右侧的「行数」菜单（重查代价随行数走，先说清）
        let preview_title = match generated.as_ref() {
            Some(info) => {
                // 排序生效时把口径说清：这是「按该列排序后的前 N 行」，不是生成顺序的前 N 行
                let head = match &preview_sort_label {
                    Some(label) => format!("预览（{label} · 前 {} 行）", preview_shown),
                    None => format!("预览（前 {} 行）", preview_shown),
                };
                format!(
                    "{head} · {} · 临时表 {} · 本次 {} 行 · {} ms",
                    info.table_name,
                    info.temp_table_name,
                    with_thousands(u64::from(info.row_count)),
                    info.elapsed_ms
                )
            }
            None if is_draft => {
                format!("预览（前 {PREVIEW_ROWS} 行）· 尚无结果——点表头「生成」")
            }
            None if dropped_by_project_switch => {
                format!("预览（前 {PREVIEW_ROWS} 行）· 所属项目已切换，结果已作废——重新生成后再看")
            }
            None => format!("预览（前 {PREVIEW_ROWS} 行）· 这一轮没有这张表的结果"),
        };
        // 段 3：预览（标题行 + 排序提示 + 表格 / 空占位）。表格 `DataTable` 自带
        // 虚拟化与横向滚动，段内直接撑满——不再随整页滚。
        let mut preview_block = div().v_flex().gap_2().w_full().flex_1().min_h_0();
        // 单表出口随表在此（生成 → 落地闭环）：作用于当前 tab 那张表，从右面板迁来。
        // 按钮 id 拼 tab 键（`DetailTarget::key`）：多个详情 tab 并存时每个 tab 各一组
        // 出口，id 必须唯一（否则 a11y 树重复节点 panic）。
        preview_block = preview_block.child(
            self.panel
                .update(cx, |panel, cx| panel.render_sinks(&tab_key, window, cx)),
        );
        preview_block = preview_block.child(
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
                        .text_color(muted)
                        .child(preview_title),
                )
                .child(if generated.is_some() {
                    self.panel.update(cx, |panel, cx| {
                        panel.render_preview_limit_menu(&tab_key, cx)
                    })
                } else {
                    div().into_any_element()
                }),
        );
        // 排序的入口提示：箭头与右键都能点，但两处都很轻，先说一句
        if preview_sort_label.is_some() {
            preview_block = preview_block.child(div().text_xs().text_color(muted).child(
                "排序是重查临时表得来的（全局前 N 行）；点表头右端的箭头可换向 / 取消，右键同一套。",
            ));
        }

        let preview = generated.as_ref().map(|info| info.preview.clone());
        match preview {
            Some(preview) if !preview.rows.is_empty() => {
                // 预览表 = 组件库 `DataTable`（与编辑器结果集同一套原语）：表头 / 列宽拖拽 /
                // 横向滚动 / 虚拟化 / 键盘选择都是组件的，本处只给容器与一个几十行的 delegate。
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
            }
            _ => {
                preview_block = preview_block.child(
                    div()
                        .flex_1()
                        .min_h(rems(ui::PREVIEW_MIN_HEIGHT))
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .child("（暂无预览）"),
                );
            }
        }

        // 三段可拖分栏（`v_resizable`，仿编辑器结果区分栏先例）：基本信息 / 列区 / 预览。
        // 每段独立滚动（列区字段多、预览表格虚拟化，各滚各的）；拖拽高度经 `on_resize`
        // 回写面板字段（rem），跨 tab / 跨窗口沿用。初始大小：基本信息 100px（可拉到 220
        // 看全进度 / 失败重试 / 来源说明），列区上次拖的高度（默认 15rem），预览吃掉剩余。
        let font_size = cx.theme().font_size;
        let columns_h = self.panel.read(cx).detail_columns_rem * font_size;
        let split_panel = self.panel.clone();
        // 分栏 id 拼 tab 键：多个详情 tab 并存时各自独立（a11y 节点 + resizable state
        // 都按 id 区分；尺寸经 on_resize 回写面板字段，跨 tab 仍沿用）
        let split = v_resizable(format!("mock-detail-split-{tab_key}"))
            .child(
                resizable_panel()
                    .size(ui::DETAIL_HEADER_HEIGHT)
                    .size_range(ui::DETAIL_HEADER_HEIGHT..ui::DETAIL_HEADER_MAX_HEIGHT)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(head_block),
                    ),
            )
            .child(
                resizable_panel()
                    .size(columns_h)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(columns_block),
                    ),
            )
            .child(
                resizable_panel()
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(preview_block),
                    ),
            )
            .on_resize(move |state, _window, app| {
                // 拖到哪记到哪：三段的第 2 段是列区（`sizes()[1]`），换算回 rem 存面板字段
                if let Some(h) = state.read(app).sizes().get(1).copied() {
                    let ratio = h / font_size;
                    split_panel.update(app, |panel, cx| {
                        panel.detail_columns_rem = ratio;
                        cx.notify();
                    });
                }
            });

        // ── 主体布局 ──
        // 草稿 tab：左栏常驻生成器库（分组 / 最近 / 搜索 / 自定义管理，方便直接调整已有
        // 生成器细节），右栏三态——编辑列 → 列编辑表单；左栏「＋」/「编辑」→ 自定义精调
        // 表单；平时 → 原三段可拖分栏。结果表 tab：只读事实，保持原三段分栏（无编辑入口）。
        let content: gpui_kit::AnyElement = if is_draft {
            let view = cx.entity();
            let panel = self.panel.clone();
            let library = self.panel.read(cx).library();
            // 右栏三态（按优先级：列编辑 > 自定义精调 > 三段）
            let right: gpui_kit::AnyElement = if self.draft.is_some() {
                let draft = self.draft.as_ref().expect("上面判过 Some");
                render_column_editor(&view, draft, &panel, cx)
            } else if let Some(lib) = library.as_ref().filter(|lib| {
                lib.read(cx).mode == crate::mock_view::generator_library::LibraryMode::Create
            }) {
                let (form_body, form_footer) =
                    crate::mock_view::generator_library::render_state(lib, window, cx);
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(form_body)
                    .when_some(form_footer, |col, footer| col.child(footer))
                    .into_any_element()
            } else {
                split.into_any_element()
            };
            // 左栏：常驻生成器库（宽 300，右缘分隔线；浏览列表超高时整体滚动）
            let theme_lib = cx.theme().clone();
            div()
                .h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .size_full()
                .child(
                    div()
                        .flex_none()
                        .w(px(300.))
                        .h_full()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .border_r_1()
                        .border_color(border)
                        .when_some(library, |col, lib| {
                            col.child(
                                crate::mock_view::generator_library::render_browse(
                                    &lib, &theme_lib, window, cx,
                                ),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(right),
                )
                .into_any_element()
        } else {
            split.into_any_element()
        };
        body = body.child(content);

        // 键位落点：`track_focus` 不能省（没有它这条 tab 不在 dispatch path 上，快捷键落不到动作）；
        // `key_context("mock-detail")` 与 `crates/app` 的键位注册（Ctrl+Enter = 生成）成对。
        body.track_focus(&self.focus_handle)
            .key_context("mock-detail")
            .on_action(cx.listener(Self::on_generate_key))
    }
}

/// 右栏列编辑表单（主界面内嵌，不弹对话框）：列名 / 类型 / 参数 / 样例 / 空值率 / 唯一 /
/// 恢复智能默认 + 底部「取消 / 应用」。仅在草稿 tab 渲染（结果表 tab 无编辑入口），
/// 元素 id 不拼 tab 键也不会与同窗口其他 tab 重复。
fn render_column_editor(
    view: &Entity<MockDetailView>,
    draft: &ColumnDraft,
    panel: &Entity<MockPanel>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme().clone();
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
    let sample_preview = draft.sample_preview.clone();

    let mut body = div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(form_line(&theme, "列名", &name_state));
    // 类型下拉
    body = body.child(
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
                    .child("类型"),
            )
            .child({
                let view = view.clone();
                Button::new("mock-col-type")
                    .ghost()
                    .label(type_label.clone())
                    .dropdown_caret(true)
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
                                                draft.type_label = column_type_label(&chosen);
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
    // 类型与生成器解耦的说明：改类型不自动换生成器（避免参数行与生成器不一致）；
    // 想按新类型重推生成器 → 点下方「恢复智能默认」
    body = body.child(
        div()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("改类型不会自动换生成器；按新类型重推请点「恢复智能默认」。"),
    );
    for row in param_rows {
        body = body.child(match row {
            ParamRowView::Scalar { label, state } => form_line(&theme, &label, &state),
            ParamRowView::Complex { label, hint, state } => {
                complex_form_line(&theme, &label, hint, &state)
            }
        });
    }
    if let Some((_, message)) = param_error {
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.colors.danger)
                .child(message),
        );
    }
    // 参数实时样例预览（2 条示意值）：输入参数即改即看，固定种子可复现
    body = body.child(
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
                    .child("样例预览"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(sample_preview),
            ),
    );
    body = body.child(form_line(&theme, "空值率（%）", &null_state));
    body = body.child(
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
                    .child("唯一值"),
            )
            .child({
                let view = view.clone();
                Switch::new("mock-col-unique").checked(unique).on_click(
                    move |checked, _window, app| {
                        let checked = *checked;
                        view.update(app, |view, cx| {
                            if let Some(draft) = view.draft.as_mut() {
                                draft.def.unique = checked;
                            }
                            cx.notify();
                        });
                    },
                )
            }),
    );
    body = body.child({
        let view = view.clone();
        Button::new("mock-col-reset")
            .secondary()
            .label("恢复智能默认")
            .on_click(move |_, window, app| {
                view.update(app, |view, cx| {
                    if let Some(draft) = view.draft.as_mut() {
                        let mapped = ColumnMapper::infer(&draft.def.name, &draft.def.data_type);
                        draft.def.generator = mapped.generator;
                        draft.def.nullable_ratio = 0.0;
                        draft.def.unique = false;
                        draft.confidence = mapped.confidence;
                        draft.sample_value = mapped.sample_value;
                    }
                    // 生成器身份可能变了：参数行按新生成器重建（事件路径，不能在 render 做）
                    cx.defer_in(window, |view, window, cx| {
                        view.rebuild_params(window, cx);
                    });
                    cx.notify();
                });
            })
    });
    // 底部动作行：取消（丢弃草稿回三段）/ 应用（合成列规格写回面板）
    let view_apply = view.clone();
    let view_cancel = view.clone();
    let panel_apply = panel.clone();
    body = body.child(
        div()
            .h_flex()
            .justify_end()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                Button::new("mock-col-cancel")
                    .secondary()
                    .label("取消")
                    .on_click(move |_, _window, app| {
                        view_cancel.update(app, |view, cx| view.close_column_editor(cx));
                    }),
            )
            .child(
                Button::new("mock-col-apply")
                    .with_variant(ButtonVariant::Primary)
                    .label("应用")
                    .on_click(move |_, _window, app| {
                        let edited = view_apply.update(app, |view, cx| view.collect_column_edit(cx));
                        if let Some(edited) = edited {
                            panel_apply.update(app, |panel, cx| panel.apply_column(edited, cx));
                        }
                        view_apply.update(app, |view, cx| view.close_column_editor(cx));
                    }),
            ),
    );
    body.into_any_element()
}

/// 清单行右端的**状态点**：`IconName` 图标（走图标组件，不再用 Unicode 字形）+ 语义色文字。
pub(crate) fn render_status_cell(icon: Option<IconName>, text: &str, color: Hsla) -> Div {
    div()
        .flex_none()
        .text_xs()
        .text_color(color)
        .when_some(icon, |cell, icon| {
            cell.child(Icon::new(icon).size(rems(ui::STATUS_ICON_SIZE)).text_color(color))
                .child(" ")
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

    /// tab 被激活 = 这张表成为「当前表」：出口（落库 / 追加 / 导出）都作用于它。
    ///
    /// 这就是「切 tab 就是切表」的落地点：用户不必再去面板里选一次。
    /// 草稿 tab 不动当前表（草稿不是结果表）。
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !active {
            return;
        }
        let DetailTarget::Table(name) = self.target.clone() else {
            return;
        };
        self.panel
            .update(cx, |panel, cx| panel.focus_table(&name, cx));
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

